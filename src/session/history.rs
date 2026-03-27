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
        self.total_tokens = stored
            .iter()
            .filter_map(|m| m.tokens)
            .sum();
        self.messages = stored;
        self.session_id = sid;
        Ok(())
    }

    /// Append a message and persist it to the store.
    ///
    /// `tokens` is an optional approximate token count for the message.
    pub fn push(&mut self, role: &str, content: &str, tokens: Option<u32>) -> Result<()> {
        self.store.save_message(&self.session_id, role, content, tokens)?;

        // Reload the last message so we have the generated ID and timestamp.
        let all = self.store.load_messages(&self.session_id)?;
        if let Some(last) = all.into_iter().last() {
            if let Some(t) = last.tokens {
                self.total_tokens += t;
            }
            self.messages.push(last);
        }

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
    pub fn total_tokens(&self) -> u32 {
        self.total_tokens
    }

    /// Number of messages in the in-memory buffer.
    pub fn len(&self) -> usize {
        self.messages.len()
    }

    /// Whether there are no messages.
    pub fn is_empty(&self) -> bool {
        self.messages.is_empty()
    }

    /// Clear in-memory messages (does **not** delete from the store).
    pub fn clear(&mut self) {
        self.messages.clear();
        self.total_tokens = 0;
    }
}
