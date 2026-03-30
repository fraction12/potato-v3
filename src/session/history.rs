//! Message history — in-memory ordered buffer backed by the session store.
//!
//! [`MessageHistory`] keeps an in-memory copy of the active session's messages
//! and writes through to [`SessionStore`] on every [`push`](MessageHistory::push).

use anyhow::Result;

use super::store::{SessionStore, StoredMessage};

/// In-memory message buffer for the active session.
///
/// All mutations persist immediately to the underlying [`SessionStore`].
pub struct MessageHistory<'a> {
    /// The active session identifier.
    session_id: String,
    /// Ordered, in-memory copy of the session's messages.
    messages: Vec<StoredMessage>,
    /// Reference to the backing store.
    store: &'a SessionStore,
    /// Running total of tokens across all messages (approximate).
    total_tokens: u32,
}

impl<'a> MessageHistory<'a> {
    /// Create a new, empty history attached to `session_id`.
    pub fn new(session_id: impl Into<String>, store: &'a SessionStore) -> Self {
        Self {
            session_id: session_id.into(),
            messages: Vec::new(),
            store,
            total_tokens: 0,
        }
    }

    /// Load an existing session's messages from the store into memory.
    ///
    /// Replaces any in-memory messages with the stored ones.
    pub fn load_session(&mut self, session_id: impl Into<String>) -> Result<()> {
        let sid = session_id.into();
        let stored = self.store.load_messages(&sid)?;
        self.total_tokens = stored.iter().filter_map(|m| m.tokens).sum();
        self.messages = stored;
        self.session_id = sid;
        Ok(())
    }

    /// Append a message and persist it to the store.
    ///
    /// `tokens` is an optional approximate token count for the message.
    pub fn push(&mut self, role: &str, content: &str, tokens: Option<u32>) -> Result<()> {
        let msg = self
            .store
            .save_message(&self.session_id, role, content, tokens)?;
        if let Some(t) = msg.tokens {
            self.total_tokens += t;
        }
        self.messages.push(msg);
        Ok(())
    }

    /// Return a slice of all in-memory messages.
    pub fn messages(&self) -> &[StoredMessage] {
        &self.messages
    }

    /// The active session ID.
    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    /// Approximate running total of tokens across all messages.
    #[must_use]
    pub fn total_tokens(&self) -> u32 {
        self.total_tokens
    }

    /// Number of messages in the in-memory buffer.
    #[must_use]
    pub fn len(&self) -> usize {
        self.messages.len()
    }

    /// Whether there are no messages.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.messages.is_empty()
    }

    /// Clear in-memory messages (does **not** delete from the store).
    pub fn clear(&mut self) {
        self.messages.clear();
        self.total_tokens = 0;
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::store::SessionStore;

    fn setup() -> (SessionStore, String) {
        let store = SessionStore::in_memory().expect("in-memory store");
        let session_id = store.create_session("test session").expect("create");
        (store, session_id)
    }

    #[test]
    fn test_push_and_messages() {
        let (store, session_id) = setup();
        let mut history = MessageHistory::new(&session_id, &store);

        history.push("user", "Hello!", None).expect("push user");
        history
            .push("assistant", "Hi there.", None)
            .expect("push assistant");

        assert_eq!(history.len(), 2);
        assert_eq!(history.messages()[0].role, "user");
        assert_eq!(history.messages()[0].content, "Hello!");
        assert_eq!(history.messages()[1].role, "assistant");
    }

    #[test]
    fn test_total_tokens() {
        let (store, session_id) = setup();
        let mut history = MessageHistory::new(&session_id, &store);

        history.push("user", "msg1", Some(10)).expect("push 1");
        history.push("assistant", "msg2", Some(20)).expect("push 2");

        assert_eq!(history.total_tokens(), 30);
    }
}
