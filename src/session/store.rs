//! Session store — SQLite-backed persistence for sessions and messages.

use anyhow::Result;
use rusqlite::Connection;

/// Manages the SQLite database that persists all session data.
pub struct SessionStore {
    conn: Connection,
}

impl SessionStore {
    /// Open (or create) the session database at the given path.
    pub fn open(path: &str) -> Result<Self> {
        let conn = Connection::open(path)?;
        let store = Self { conn };
        store.migrate()?;
        Ok(store)
    }

    /// Open an in-memory database (useful for tests).
    pub fn in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        let store = Self { conn };
        store.migrate()?;
        Ok(store)
    }

    /// Apply any pending schema migrations.
    fn migrate(&self) -> Result<()> {
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS sessions (
                id      TEXT PRIMARY KEY,
                title   TEXT NOT NULL DEFAULT '',
                created INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS messages (
                id         TEXT PRIMARY KEY,
                session_id TEXT NOT NULL,
                role       TEXT NOT NULL,
                content    TEXT NOT NULL,
                created    INTEGER NOT NULL,
                FOREIGN KEY (session_id) REFERENCES sessions(id)
            );",
        )?;
        Ok(())
    }

    /// Return the underlying connection (for advanced queries).
    pub fn connection(&self) -> &Connection {
        &self.conn
    }
}
