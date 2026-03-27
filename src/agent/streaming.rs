//! Streaming token accumulator — buffers partial tokens into complete chunks.

/// Accumulates streamed text tokens and emits complete logical chunks.
#[derive(Debug, Default)]
pub struct StreamAccumulator {
    /// Internal buffer of received partial content.
    buffer: String,
    /// Total tokens received so far.
    pub total_tokens: u64,
}

impl StreamAccumulator {
    /// Create a new empty [`StreamAccumulator`].
    pub fn new() -> Self {
        Self::default()
    }

    /// Push a raw token fragment into the accumulator.
    ///
    /// Returns `Some(chunk)` if a complete chunk is ready to emit.
    pub fn push(&mut self, token: impl Into<String>) -> Option<String> {
        let t = token.into();
        self.total_tokens += 1;
        self.buffer.push_str(&t);
        // Stub: emit every token immediately.
        let chunk = self.buffer.clone();
        self.buffer.clear();
        Some(chunk)
    }

    /// Flush any remaining buffered content.
    pub fn flush(&mut self) -> Option<String> {
        if self.buffer.is_empty() {
            None
        } else {
            let chunk = self.buffer.clone();
            self.buffer.clear();
            Some(chunk)
        }
    }
}
