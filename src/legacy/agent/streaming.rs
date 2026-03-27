//! Streaming token accumulator — buffers partial tokens into complete logical chunks.

/// Accumulates streamed text tokens and emits complete logical chunks.
///
/// Currently emits each token immediately; the accumulator boundary logic can be
/// extended (e.g. word-level, sentence-level) without changing the public API.
#[derive(Debug, Default)]
pub struct StreamAccumulator {
    /// Internal buffer of received partial content that has not yet been emitted.
    buffer: String,
    /// Total number of raw token fragments pushed so far.
    pub total_tokens: u64,
    /// Cumulative character count of all content emitted.
    pub total_chars: u64,
}

impl StreamAccumulator {
    /// Create a new empty [`StreamAccumulator`].
    pub fn new() -> Self {
        Self::default()
    }

    /// Push a raw token fragment into the accumulator.
    ///
    /// Returns `Some(chunk)` with the content to forward to the UI, or `None`
    /// if the fragment should be buffered (e.g. for boundary detection in future
    /// implementations).  The current policy is to emit every non-empty fragment
    /// immediately.
    pub fn push(&mut self, token: impl Into<String>) -> Option<String> {
        let t = token.into();
        if t.is_empty() {
            return None;
        }
        self.total_tokens += 1;
        self.total_chars += t.len() as u64;
        self.buffer.push_str(&t);

        // Emit-immediately policy: drain the buffer on every push.
        let chunk = self.buffer.clone();
        self.buffer.clear();
        Some(chunk)
    }

    /// Flush any content that remains in the buffer after the stream ends.
    ///
    /// Should be called once after the `done: true` sentinel is received.
    pub fn flush(&mut self) -> Option<String> {
        if self.buffer.is_empty() {
            None
        } else {
            let chunk = self.buffer.clone();
            self.buffer.clear();
            Some(chunk)
        }
    }

    /// Reset accumulator state for a new turn.
    pub fn reset(&mut self) {
        self.buffer.clear();
        self.total_tokens = 0;
        self.total_chars = 0;
    }

    /// Return the number of characters buffered but not yet emitted.
    pub fn buffered_len(&self) -> usize {
        self.buffer.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emits_every_token() {
        let mut acc = StreamAccumulator::new();
        assert_eq!(acc.push("hello"), Some("hello".to_string()));
        assert_eq!(acc.push(" world"), Some(" world".to_string()));
        assert_eq!(acc.total_tokens, 2);
    }

    #[test]
    fn empty_token_returns_none() {
        let mut acc = StreamAccumulator::new();
        assert_eq!(acc.push(""), None);
        assert_eq!(acc.total_tokens, 0);
    }

    #[test]
    fn flush_returns_buffered_content() {
        let mut acc = StreamAccumulator::new();
        // Directly write to buffer to simulate partial-buffering future policy.
        acc.buffer.push_str("partial");
        assert_eq!(acc.flush(), Some("partial".to_string()));
        assert_eq!(acc.flush(), None);
    }

    #[test]
    fn reset_clears_state() {
        let mut acc = StreamAccumulator::new();
        acc.push("data");
        acc.reset();
        assert_eq!(acc.total_tokens, 0);
        assert_eq!(acc.total_chars, 0);
    }
}
