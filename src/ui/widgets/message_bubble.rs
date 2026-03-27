//! Message bubble widget — renders a single chat message with role styling.

/// Renders a single user or assistant message as a styled bubble.
#[derive(Debug, Default)]
pub struct MessageBubble {
    /// The text content of the message.
    pub content: String,
    /// Whether this message is from the user (true) or assistant (false).
    pub is_user: bool,
}

impl MessageBubble {
    /// Create a new [`MessageBubble`].
    pub fn new(content: impl Into<String>, is_user: bool) -> Self {
        Self {
            content: content.into(),
            is_user,
        }
    }
}
