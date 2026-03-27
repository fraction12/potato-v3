//! Message history — in-memory ordered list of messages for the active session.

use crate::ollama::types::ChatMessage;

/// Maintains the ordered list of messages for a single session.
#[derive(Debug, Default)]
pub struct MessageHistory {
    messages: Vec<ChatMessage>,
    /// Maximum messages to retain (0 = unlimited).
    pub max_messages: usize,
}

impl MessageHistory {
    /// Create an empty history.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a history with a maximum retention limit.
    pub fn with_limit(max_messages: usize) -> Self {
        Self {
            max_messages,
            ..Default::default()
        }
    }

    /// Append a message to the history.
    pub fn push(&mut self, msg: ChatMessage) {
        if self.max_messages > 0 && self.messages.len() >= self.max_messages {
            self.messages.remove(0);
        }
        self.messages.push(msg);
    }

    /// Return a slice of all messages.
    pub fn messages(&self) -> &[ChatMessage] {
        &self.messages
    }

    /// Number of messages in the history.
    pub fn len(&self) -> usize {
        self.messages.len()
    }

    /// Whether the history is empty.
    pub fn is_empty(&self) -> bool {
        self.messages.is_empty()
    }

    /// Clear all messages.
    pub fn clear(&mut self) {
        self.messages.clear();
    }
}
