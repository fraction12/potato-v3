//! Project-scoped persistent state — `.potato/state.db`.
//!
//! Persists shared context and task board across Potato sessions.
//! Roles, inboxes, and pane registrations are runtime-only (ephemeral).

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use rusqlite::{Connection, params};
use serde_json::Value;

use super::state::{TaskClaim, MessagePriority, InterMessage};

/// SQLite-backed project store at `.potato/state.db`.
#[derive(Debug)]
pub struct ProjectStore {
    conn: Connection,
    #[allow(dead_code)]
    path: PathBuf,
}

impl ProjectStore {
    /// Open or create the project store at `<project_root>/.potato/state.db`.
    pub fn open(project_root: &Path) -> Result<Self> {
        let dir = project_root.join(".potato");
        std::fs::create_dir_all(&dir)
            .with_context(|| format!("failed to create .potato/ in {}", project_root.display()))?;

        let db_path = dir.join("state.db");
        let conn = Connection::open(&db_path)
            .with_context(|| format!("failed to open {}", db_path.display()))?;

        let store = Self { conn, path: db_path };
        store.configure()?;
        store.migrate()?;
        Ok(store)
    }

    /// In-memory store for tests.
    pub fn in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()
            .context("failed to open in-memory project store")?;
        let store = Self { conn, path: PathBuf::from(":memory:") };
        store.configure()?;
        store.migrate()?;
        Ok(store)
    }

    fn configure(&self) -> Result<()> {
        self.conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA foreign_keys=ON;",
        ).context("failed to configure project store pragmas")
    }

    fn migrate(&self) -> Result<()> {
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS shared_context (
                key   TEXT PRIMARY KEY,
                value TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS task_board (
                task_id     TEXT PRIMARY KEY,
                description TEXT NOT NULL,
                status      TEXT NOT NULL DEFAULT 'open',
                claimed_by  INTEGER,
                claimed_at  TEXT,
                completed_at TEXT,
                updated_at  TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS message_log (
                id         INTEGER PRIMARY KEY AUTOINCREMENT,
                from_pane  INTEGER NOT NULL,
                to_pane    INTEGER NOT NULL,
                content    TEXT NOT NULL,
                priority   TEXT NOT NULL DEFAULT 'normal',
                created_at TEXT NOT NULL
            );"
        ).context("failed to run project store migrations")
    }

    // ── Shared context ────────────────────────────────────────────────────────

    /// Upsert a shared context key-value pair.
    pub fn set_context(&self, key: &str, value: &Value) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        let json = serde_json::to_string(value)?;
        self.conn.execute(
            "INSERT INTO shared_context (key, value, updated_at)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(key) DO UPDATE SET value = ?2, updated_at = ?3",
            params![key, json, now],
        )?;
        Ok(())
    }

    /// Delete a shared context key.
    pub fn delete_context(&self, key: &str) -> Result<bool> {
        let rows = self.conn.execute(
            "DELETE FROM shared_context WHERE key = ?1",
            params![key],
        )?;
        Ok(rows > 0)
    }

    /// Load all shared context entries.
    pub fn load_context(&self) -> Result<Vec<(String, Value)>> {
        let mut stmt = self.conn.prepare(
            "SELECT key, value FROM shared_context ORDER BY key"
        )?;
        let rows = stmt.query_map([], |row| {
            let key: String = row.get(0)?;
            let json: String = row.get(1)?;
            Ok((key, json))
        })?;

        let mut result = Vec::new();
        for row in rows {
            let (key, json) = row?;
            if let Ok(value) = serde_json::from_str(&json) {
                result.push((key, value));
            }
        }
        Ok(result)
    }

    // ── Task board ────────────────────────────────────────────────────────────

    /// Upsert a task (claimed by a pane).
    pub fn upsert_task(&self, task_id: &str, description: &str, claimed_by: u64) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            "INSERT INTO task_board (task_id, description, status, claimed_by, claimed_at, updated_at)
             VALUES (?1, ?2, 'active', ?3, ?4, ?4)
             ON CONFLICT(task_id) DO UPDATE SET
                description = ?2, status = 'active', claimed_by = ?3, claimed_at = ?4, updated_at = ?4",
            params![task_id, description, claimed_by as i64, now],
        )?;
        Ok(())
    }

    /// Release a task (set status to open, clear claimer).
    pub fn release_task(&self, task_id: &str) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            "UPDATE task_board SET status = 'open', claimed_by = NULL, claimed_at = NULL, updated_at = ?2
             WHERE task_id = ?1",
            params![task_id, now],
        )?;
        Ok(())
    }

    /// Mark a task as completed.
    pub fn complete_task(&self, task_id: &str) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            "UPDATE task_board SET status = 'completed', completed_at = ?2, updated_at = ?2
             WHERE task_id = ?1",
            params![task_id, now],
        )?;
        Ok(())
    }

    /// Load all non-completed tasks.
    pub fn load_tasks(&self) -> Result<Vec<TaskClaim>> {
        let mut stmt = self.conn.prepare(
            "SELECT task_id, description, claimed_by, claimed_at
             FROM task_board WHERE status != 'completed' ORDER BY task_id"
        )?;
        let rows = stmt.query_map([], |row| {
            let task_id: String = row.get(0)?;
            let description: String = row.get(1)?;
            let claimed_by: Option<i64> = row.get(2)?;
            let claimed_at: Option<String> = row.get(3)?;
            Ok((task_id, description, claimed_by, claimed_at))
        })?;

        let mut result = Vec::new();
        for row in rows {
            let (task_id, description, claimed_by, claimed_at) = row?;
            if let Some(pane_id) = claimed_by {
                let ts = claimed_at
                    .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
                    .map(|dt| dt.with_timezone(&Utc))
                    .unwrap_or_else(Utc::now);
                result.push(TaskClaim {
                    task_id,
                    description,
                    claimed_by: pane_id as u64,
                    claimed_at: ts,
                });
            }
        }
        Ok(result)
    }

    // ── Message log (append-only history) ─────────────────────────────────────

    /// Log a message for history/audit. Not used for live delivery.
    pub fn log_message(&self, msg: &InterMessage, to_pane: u64) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        let priority = match msg.priority {
            MessagePriority::Normal => "normal",
            MessagePriority::Urgent => "urgent",
        };
        self.conn.execute(
            "INSERT INTO message_log (from_pane, to_pane, content, priority, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![msg.from_pane as i64, to_pane as i64, msg.content, priority, now],
        )?;
        Ok(())
    }

    /// Get recent message history (most recent first).
    pub fn recent_messages(&self, limit: usize) -> Result<Vec<(u64, u64, String, String)>> {
        let mut stmt = self.conn.prepare(
            "SELECT from_pane, to_pane, content, created_at
             FROM message_log ORDER BY id DESC LIMIT ?1"
        )?;
        let rows = stmt.query_map(params![limit as i64], |row| {
            let from: i64 = row.get(0)?;
            let to: i64 = row.get(1)?;
            let content: String = row.get(2)?;
            let created: String = row.get(3)?;
            Ok((from as u64, to as u64, content, created))
        })?;

        let mut result = Vec::new();
        for row in rows {
            result.push(row?);
        }
        Ok(result)
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn store() -> ProjectStore {
        ProjectStore::in_memory().unwrap()
    }

    #[test]
    fn context_roundtrip() {
        let s = store();
        s.set_context("schema", &json!({"version": 2})).unwrap();
        s.set_context("plan", &json!("build the thing")).unwrap();

        let ctx = s.load_context().unwrap();
        assert_eq!(ctx.len(), 2);
        assert_eq!(ctx[0].0, "plan");
        assert_eq!(ctx[1].0, "schema");
        assert_eq!(ctx[1].1, json!({"version": 2}));
    }

    #[test]
    fn context_upsert_overwrites() {
        let s = store();
        s.set_context("key", &json!("v1")).unwrap();
        s.set_context("key", &json!("v2")).unwrap();
        let ctx = s.load_context().unwrap();
        assert_eq!(ctx.len(), 1);
        assert_eq!(ctx[0].1, json!("v2"));
    }

    #[test]
    fn context_delete() {
        let s = store();
        s.set_context("key", &json!("val")).unwrap();
        assert!(s.delete_context("key").unwrap());
        assert!(!s.delete_context("key").unwrap());
        assert!(s.load_context().unwrap().is_empty());
    }

    #[test]
    fn task_roundtrip() {
        let s = store();
        s.upsert_task("t-1", "fix bug", 0).unwrap();
        s.upsert_task("t-2", "add feature", 1).unwrap();

        let tasks = s.load_tasks().unwrap();
        assert_eq!(tasks.len(), 2);
        assert_eq!(tasks[0].task_id, "t-1");
        assert_eq!(tasks[0].claimed_by, 0);
    }

    #[test]
    fn task_release_and_complete() {
        let s = store();
        s.upsert_task("t-1", "fix bug", 0).unwrap();

        s.release_task("t-1").unwrap();
        // Released tasks are 'open' with no claimer — load_tasks only
        // returns tasks with a claimer.
        assert!(s.load_tasks().unwrap().is_empty());

        s.upsert_task("t-1", "fix bug", 1).unwrap();
        s.complete_task("t-1").unwrap();
        // Completed tasks are excluded.
        assert!(s.load_tasks().unwrap().is_empty());
    }

    #[test]
    fn message_log() {
        let s = store();
        let msg = InterMessage {
            from_pane: 0,
            content: "hello".into(),
            priority: MessagePriority::Normal,
            timestamp: Utc::now(),
            read: false,
        };
        s.log_message(&msg, 1).unwrap();

        let recent = s.recent_messages(10).unwrap();
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].0, 0); // from
        assert_eq!(recent[0].1, 1); // to
        assert_eq!(recent[0].2, "hello");
    }
}
