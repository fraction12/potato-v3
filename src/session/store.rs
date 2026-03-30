//! Session store — SQLite-backed persistence for cockpit sessions.
//!
//! Opens (or creates) a database at the given path, enables WAL mode for
//! better concurrent read performance, and provides CRUD operations for
//! sessions and their events.

use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use rusqlite::{Connection, params};

// ── Public data types ─────────────────────────────────────────────────────────

/// A single persisted message row (kept for backward compatibility).
#[derive(Debug, Clone)]
pub struct StoredMessage {
    /// Unique message identifier.
    pub id: String,
    /// The session this message belongs to.
    pub session_id: String,
    /// Role: "user", "assistant", "system", or "tool".
    pub role: String,
    /// Message content.
    pub content: String,
    /// Unix timestamp (seconds) at creation.
    pub created_at: i64,
    /// Optional token count for this message.
    pub tokens: Option<u32>,
}

/// Summary row returned by [`SessionStore::list_sessions`].
#[derive(Debug, Clone)]
pub struct SessionInfo {
    /// Session identifier (Claude native UUID).
    pub id: String,
    /// The Claude project directory name (e.g. "-Users-dushyant-jarvis-…").
    pub project_dir: String,
    /// Human-readable title (from first user prompt).
    pub title: String,
    /// Agent name (e.g. "claude").
    pub agent: String,
    /// Model name if known.
    pub model: Option<String>,
    /// Total input tokens accumulated.
    pub total_input_tokens: u64,
    /// Total output tokens accumulated.
    pub total_output_tokens: u64,
    /// Number of assistant turns.
    pub turn_count: u64,
    /// Unix timestamp (seconds) at creation.
    pub created_at: i64,
    /// Unix timestamp (seconds) at last update.
    pub updated_at: i64,
}

impl SessionInfo {
    /// Total tokens (input + output).
    #[must_use]
    pub fn total_tokens(&self) -> u64 {
        self.total_input_tokens + self.total_output_tokens
    }
}

/// A single recorded event within a session.
#[derive(Debug, Clone)]
pub struct SessionEvent {
    pub session_id: String,
    /// "assistant", "user", "tool_use", "tool_result", "system"
    pub event_type: String,
    /// Compact preview text.
    pub summary: String,
    pub tokens_in: Option<i64>,
    pub tokens_out: Option<i64>,
    pub timestamp: i64,
}

// ── Store ─────────────────────────────────────────────────────────────────────

/// Manages the SQLite database that persists all session data.
pub struct SessionStore {
    conn: Connection,
}

impl std::fmt::Debug for SessionStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SessionStore").finish_non_exhaustive()
    }
}

impl SessionStore {
    /// Open (or create) the session database at the given path.
    ///
    /// WAL mode is enabled immediately after opening for better concurrency.
    pub fn open(path: &str) -> Result<Self> {
        let conn = Connection::open(path)
            .with_context(|| format!("failed to open session database at: {path}"))?;
        let store = Self { conn };
        store.configure()?;
        store.migrate()?;
        Ok(store)
    }

    /// Open an in-memory database — useful for tests.
    pub fn in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()
            .context("failed to open in-memory session database")?;
        let store = Self { conn };
        store.configure()?;
        store.migrate()?;
        Ok(store)
    }

    /// Apply PRAGMAs (WAL mode, foreign keys).
    fn configure(&self) -> Result<()> {
        self.conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA foreign_keys=ON;",
        )
        .context("failed to configure database pragmas")
    }

    /// Apply schema migrations — idempotent.
    ///
    /// Detects the pre-cockpit schema (old `sessions` table without
    /// `project_dir` column) and drops it so the new schema can be created.
    fn migrate(&self) -> Result<()> {
        // Check if we have the old schema (sessions table exists but lacks project_dir).
        let has_old_schema: bool = self.conn.query_row(
            "SELECT COUNT(*) FROM pragma_table_info('sessions') WHERE name = 'project_dir'",
            [],
            |row| row.get::<_, i64>(0),
        ).unwrap_or(0) == 0
        && self.conn.query_row(
            "SELECT COUNT(*) FROM pragma_table_info('sessions') WHERE name = 'title'",
            [],
            |row| row.get::<_, i64>(0),
        ).unwrap_or(0) > 0;

        if has_old_schema {
            tracing::info!("Detected pre-cockpit schema; dropping old sessions/messages tables");
            self.conn.execute_batch(
                "DROP TABLE IF EXISTS messages;
                 DROP TABLE IF EXISTS sessions;"
            ).context("failed to drop old schema")?;
        }

        self.conn.execute_batch(
            // ── Cockpit sessions table ────────────────────────────────────────
            "CREATE TABLE IF NOT EXISTS sessions (
                id                    TEXT PRIMARY KEY,
                project_dir           TEXT NOT NULL DEFAULT '',
                agent                 TEXT NOT NULL DEFAULT 'claude',
                model                 TEXT,
                title                 TEXT NOT NULL DEFAULT '',
                cwd                   TEXT,
                total_input_tokens    INTEGER NOT NULL DEFAULT 0,
                total_output_tokens   INTEGER NOT NULL DEFAULT 0,
                turn_count            INTEGER NOT NULL DEFAULT 0,
                created_at            INTEGER NOT NULL,
                updated_at            INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS session_events (
                id          INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id  TEXT NOT NULL,
                event_type  TEXT NOT NULL,
                summary     TEXT NOT NULL DEFAULT '',
                tokens_in   INTEGER,
                tokens_out  INTEGER,
                timestamp   INTEGER NOT NULL,
                FOREIGN KEY (session_id) REFERENCES sessions(id) ON DELETE CASCADE
            );

            CREATE INDEX IF NOT EXISTS idx_events_session
                ON session_events(session_id, timestamp);

            -- Legacy table kept for schema compat (not actively used).
            CREATE TABLE IF NOT EXISTS messages (
                id          TEXT PRIMARY KEY,
                session_id  TEXT NOT NULL,
                role        TEXT NOT NULL,
                content     TEXT NOT NULL,
                created     INTEGER NOT NULL,
                token_count INTEGER,
                FOREIGN KEY (session_id) REFERENCES sessions(id) ON DELETE CASCADE
            );",
        )
        .context("failed to run database migrations")
    }

    // ── Session CRUD ──────────────────────────────────────────────────────────

    /// Upsert a session row. Creates it if absent; updates totals and timestamp
    /// if it already exists but the incoming data has higher token counts.
    pub fn upsert_session(
        &self,
        id: &str,
        project_dir: &str,
        agent: &str,
        model: Option<&str>,
        title: &str,
        cwd: Option<&str>,
        total_input_tokens: u64,
        total_output_tokens: u64,
        turn_count: u64,
        created_at: i64,
        updated_at: i64,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO sessions
                (id, project_dir, agent, model, title, cwd,
                 total_input_tokens, total_output_tokens, turn_count,
                 created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
             ON CONFLICT(id) DO UPDATE SET
                model                = COALESCE(excluded.model, model),
                title                = CASE WHEN excluded.title != '' THEN excluded.title ELSE title END,
                cwd                  = COALESCE(excluded.cwd, cwd),
                total_input_tokens   = MAX(excluded.total_input_tokens, total_input_tokens),
                total_output_tokens  = MAX(excluded.total_output_tokens, total_output_tokens),
                turn_count           = MAX(excluded.turn_count, turn_count),
                updated_at           = MAX(excluded.updated_at, updated_at)",
            params![
                id,
                project_dir,
                agent,
                model,
                title,
                cwd,
                total_input_tokens as i64,
                total_output_tokens as i64,
                turn_count as i64,
                created_at,
                updated_at,
            ],
        )
        .context("failed to upsert session")?;
        Ok(())
    }

    /// List all sessions ordered by `updated_at` DESC (newest first).
    pub fn list_sessions(&self) -> Result<Vec<SessionInfo>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, project_dir, agent, model, title,
                    total_input_tokens, total_output_tokens, turn_count,
                    created_at, updated_at
             FROM sessions
             ORDER BY updated_at DESC",
        )?;

        let rows = stmt.query_map([], |row| {
            Ok(SessionInfo {
                id:                  row.get(0)?,
                project_dir:         row.get(1)?,
                agent:               row.get(2)?,
                model:               row.get(3)?,
                title:               row.get(4)?,
                total_input_tokens:  row.get::<_, i64>(5)? as u64,
                total_output_tokens: row.get::<_, i64>(6)? as u64,
                turn_count:          row.get::<_, i64>(7)? as u64,
                created_at:          row.get(8)?,
                updated_at:          row.get(9)?,
            })
        })?;

        let mut sessions = Vec::new();
        for r in rows {
            sessions.push(r.context("failed to read session row")?);
        }
        Ok(sessions)
    }

    /// Delete a session and all its events (CASCADE).
    pub fn delete_session(&self, session_id: &str) -> Result<()> {
        self.conn
            .execute("DELETE FROM sessions WHERE id = ?1", params![session_id])
            .context("failed to delete session")?;
        Ok(())
    }

    // ── Event CRUD ────────────────────────────────────────────────────────────

    /// Append a single event to the log for a session.
    pub fn append_event(&self, event: &SessionEvent) -> Result<()> {
        self.conn.execute(
            "INSERT INTO session_events
                (session_id, event_type, summary, tokens_in, tokens_out, timestamp)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                event.session_id,
                event.event_type,
                event.summary,
                event.tokens_in,
                event.tokens_out,
                event.timestamp,
            ],
        )
        .context("failed to insert session event")?;
        Ok(())
    }

    /// Count events already persisted for a session (used to detect new ones).
    pub fn event_count(&self, session_id: &str) -> Result<i64> {
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM session_events WHERE session_id = ?1",
            params![session_id],
            |row| row.get(0),
        )?;
        Ok(count)
    }

    /// Return the underlying connection (for advanced queries and tests).
    pub fn connection(&self) -> &Connection {
        &self.conn
    }

    // ── Legacy helpers (kept so existing callers compile) ─────────────────────

    /// Create a new session with the given title (legacy API, uses upsert).
    ///
    /// Returns the generated session ID.
    pub fn create_session(&self, title: &str) -> Result<String> {
        let id = new_id();
        let now = unix_now();
        self.upsert_session(&id, "", "claude", None, title, None, 0, 0, 0, now, now)?;
        Ok(id)
    }

    /// Persist a message belonging to a session (legacy API).
    pub fn save_message(
        &self,
        session_id: &str,
        role: &str,
        content: &str,
        tokens: Option<u32>,
    ) -> Result<()> {
        let id = new_id();
        let now = unix_now();
        self.conn
            .execute(
                "INSERT INTO messages (id, session_id, role, content, created, token_count)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![id, session_id, role, content, now, tokens],
            )
            .context("failed to insert message")?;
        Ok(())
    }

    /// Load all messages for a session, ordered by creation time (legacy API).
    pub fn load_messages(&self, session_id: &str) -> Result<Vec<StoredMessage>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, session_id, role, content, created, token_count
             FROM messages
             WHERE session_id = ?1
             ORDER BY created ASC",
        )?;

        let rows = stmt.query_map(params![session_id], |row| {
            Ok(StoredMessage {
                id:         row.get(0)?,
                session_id: row.get(1)?,
                role:       row.get(2)?,
                content:    row.get(3)?,
                created_at: row.get(4)?,
                tokens:     row.get(5)?,
            })
        })?;

        let mut messages = Vec::new();
        for r in rows {
            messages.push(r.context("failed to read message row")?);
        }
        Ok(messages)
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Generate a unique ID from the current nanosecond timestamp (hex-encoded).
fn new_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("{:x}", nanos)
}

/// Current Unix timestamp in whole seconds.
pub fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh_store() -> SessionStore {
        SessionStore::in_memory().expect("in-memory store")
    }

    #[test]
    fn test_upsert_and_list_sessions() {
        let store = fresh_store();
        assert!(store.list_sessions().expect("list").is_empty());

        let now = unix_now();
        store
            .upsert_session("uuid-1", "proj-a", "claude", Some("claude-3-5"), "Hello world", None, 100, 200, 3, now, now)
            .expect("upsert 1");
        store
            .upsert_session("uuid-2", "proj-b", "claude", None, "Another session", None, 0, 0, 0, now - 10, now - 5)
            .expect("upsert 2");

        let sessions = store.list_sessions().expect("list");
        assert_eq!(sessions.len(), 2);
        // uuid-1 has latest updated_at so it comes first.
        assert_eq!(sessions[0].id, "uuid-1");
        assert_eq!(sessions[0].total_input_tokens, 100);
        assert_eq!(sessions[0].total_output_tokens, 200);
        assert_eq!(sessions[0].turn_count, 3);
        assert_eq!(sessions[0].model.as_deref(), Some("claude-3-5"));
    }

    #[test]
    fn test_upsert_idempotent_and_updates_totals() {
        let store = fresh_store();
        let now = unix_now();
        store
            .upsert_session("uuid-1", "proj", "claude", None, "", None, 50, 80, 1, now, now)
            .expect("first upsert");
        // Second call with higher totals — should update.
        store
            .upsert_session("uuid-1", "proj", "claude", Some("claude-3-5"), "First prompt", None, 150, 200, 4, now, now + 5)
            .expect("second upsert");

        let sessions = store.list_sessions().expect("list");
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].total_input_tokens, 150);
        assert_eq!(sessions[0].turn_count, 4);
        assert_eq!(sessions[0].title, "First prompt");
        assert_eq!(sessions[0].model.as_deref(), Some("claude-3-5"));
    }

    #[test]
    fn test_append_and_count_events() {
        let store = fresh_store();
        let now = unix_now();
        store
            .upsert_session("uuid-1", "proj", "claude", None, "", None, 0, 0, 0, now, now)
            .expect("upsert");

        assert_eq!(store.event_count("uuid-1").expect("count"), 0);

        store
            .append_event(&SessionEvent {
                session_id: "uuid-1".into(),
                event_type: "user".into(),
                summary: "Hello".into(),
                tokens_in: None,
                tokens_out: None,
                timestamp: now,
            })
            .expect("append");

        store
            .append_event(&SessionEvent {
                session_id: "uuid-1".into(),
                event_type: "assistant".into(),
                summary: "Hi there".into(),
                tokens_in: Some(10),
                tokens_out: Some(20),
                timestamp: now + 1,
            })
            .expect("append");

        assert_eq!(store.event_count("uuid-1").expect("count"), 2);
    }

    #[test]
    fn test_delete_session() {
        let store = fresh_store();
        let now = unix_now();
        store
            .upsert_session("uuid-del", "proj", "claude", None, "Delete me", None, 0, 0, 0, now, now)
            .expect("upsert");
        assert_eq!(store.list_sessions().expect("list").len(), 1);

        store.delete_session("uuid-del").expect("delete");
        assert!(store.list_sessions().expect("list").is_empty());
    }

    #[test]
    fn test_legacy_create_and_list() {
        // Ensures old code paths still compile and work.
        let store = fresh_store();
        let _id1 = store.create_session("Old Session One").expect("create 1");
        let _id2 = store.create_session("Old Session Two").expect("create 2");
        let sessions = store.list_sessions().expect("list");
        assert_eq!(sessions.len(), 2);
    }

    #[test]
    fn test_legacy_save_and_load_messages() {
        let store = fresh_store();
        let session_id = store.create_session("Test Session").expect("create");
        store.save_message(&session_id, "user", "Hello!", Some(5)).expect("save user");
        store.save_message(&session_id, "assistant", "Hi there.", Some(8)).expect("save assistant");
        let messages = store.load_messages(&session_id).expect("load");
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].role, "user");
        assert_eq!(messages[0].content, "Hello!");
        assert_eq!(messages[0].tokens, Some(5));
        assert_eq!(messages[1].role, "assistant");
    }

    #[test]
    fn session_info_total_tokens() {
        let now = unix_now();
        let store = fresh_store();
        store
            .upsert_session("x", "p", "claude", None, "t", None, 300, 700, 5, now, now)
            .expect("upsert");
        let sessions = store.list_sessions().expect("list");
        assert_eq!(sessions[0].total_tokens(), 1000);
    }
}
