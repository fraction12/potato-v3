//! Session store — SQLite-backed persistence for sessions and messages.
//!
//! Opens (or creates) a database at the given path, enables WAL mode for
//! better concurrent read performance, and provides CRUD operations for
//! sessions and their messages.

use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use rusqlite::{Connection, params};

// ── Public data types ─────────────────────────────────────────────────────────

/// A single persisted message row.
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
    /// Session identifier.
    pub id: String,
    /// Human-readable title.
    pub title: String,
    /// Unix timestamp (seconds) at creation.
    pub created_at: i64,
    /// Total number of messages in this session.
    pub message_count: usize,
}

// ── Store ─────────────────────────────────────────────────────────────────────

/// Manages the SQLite database that persists all session data.
pub struct SessionStore {
    conn: Connection,
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
    fn migrate(&self) -> Result<()> {
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS sessions (
                id      TEXT PRIMARY KEY,
                title   TEXT NOT NULL DEFAULT '',
                created INTEGER NOT NULL
            );
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

    /// Create a new session with the given title.
    ///
    /// Returns the generated session ID.
    pub fn create_session(&self, title: &str) -> Result<String> {
        let id = new_id();
        let now = unix_now();
        self.conn
            .execute(
                "INSERT INTO sessions (id, title, created) VALUES (?1, ?2, ?3)",
                params![id, title, now],
            )
            .context("failed to insert session")?;
        Ok(id)
    }

    /// List all sessions ordered by creation time (newest first).
    pub fn list_sessions(&self) -> Result<Vec<SessionInfo>> {
        let mut stmt = self.conn.prepare(
            "SELECT s.id, s.title, s.created,
                    (SELECT COUNT(*) FROM messages m WHERE m.session_id = s.id) AS msg_count
             FROM sessions s
             ORDER BY s.created DESC",
        )?;

        let rows = stmt.query_map([], |row| {
            Ok(SessionInfo {
                id: row.get(0)?,
                title: row.get(1)?,
                created_at: row.get(2)?,
                message_count: row.get::<_, i64>(3)? as usize,
            })
        })?;

        let mut sessions = Vec::new();
        for r in rows {
            sessions.push(r.context("failed to read session row")?);
        }
        Ok(sessions)
    }

    /// Delete a session and all its messages (CASCADE).
    pub fn delete_session(&self, session_id: &str) -> Result<()> {
        self.conn
            .execute("DELETE FROM sessions WHERE id = ?1", params![session_id])
            .context("failed to delete session")?;
        Ok(())
    }

    // ── Message CRUD ──────────────────────────────────────────────────────────

    /// Persist a message belonging to a session.
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

    /// Load all messages for a session, ordered by creation time.
    pub fn load_messages(&self, session_id: &str) -> Result<Vec<StoredMessage>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, session_id, role, content, created, token_count
             FROM messages
             WHERE session_id = ?1
             ORDER BY created ASC",
        )?;

        let rows = stmt.query_map(params![session_id], |row| {
            Ok(StoredMessage {
                id: row.get(0)?,
                session_id: row.get(1)?,
                role: row.get(2)?,
                content: row.get(3)?,
                created_at: row.get(4)?,
                tokens: row.get(5)?,
            })
        })?;

        let mut messages = Vec::new();
        for r in rows {
            messages.push(r.context("failed to read message row")?);
        }
        Ok(messages)
    }

    /// Return the underlying connection (for advanced queries and tests).
    pub fn connection(&self) -> &Connection {
        &self.conn
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
fn unix_now() -> i64 {
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
    fn test_create_and_list_sessions() {
        let store = fresh_store();
        assert!(store.list_sessions().expect("list").is_empty());

        let id1 = store.create_session("Session One").expect("create 1");
        let id2 = store.create_session("Session Two").expect("create 2");

        let sessions = store.list_sessions().expect("list");
        assert_eq!(sessions.len(), 2);

        // IDs should be present.
        let ids: Vec<&str> = sessions.iter().map(|s| s.id.as_str()).collect();
        assert!(ids.contains(&id1.as_str()));
        assert!(ids.contains(&id2.as_str()));
    }

    #[test]
    fn test_save_and_load_messages() {
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
    fn test_delete_session() {
        let store = fresh_store();
        let id = store.create_session("To Delete").expect("create");
        assert_eq!(store.list_sessions().expect("list").len(), 1);

        store.delete_session(&id).expect("delete");
        assert!(store.list_sessions().expect("list").is_empty());
    }
}
