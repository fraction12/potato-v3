//! Token sparkline widget — mini time-series chart for token usage.

/// Renders a compact sparkline of recent token counts.
#[derive(Debug, Default)]
pub struct TokenSparkline {
    /// Ring buffer of recent token counts (newest last).
    pub data: Vec<u64>,
    /// Maximum capacity of the ring buffer.
    pub capacity: usize,
}

impl TokenSparkline {
    /// Create a new [`TokenSparkline`] with the given history capacity.
    pub fn new(capacity: usize) -> Self {
        Self {
            data: Vec::with_capacity(capacity),
            capacity,
        }
    }

    /// Push a new token count sample.
    pub fn push(&mut self, value: u64) {
        if self.data.len() >= self.capacity {
            self.data.remove(0);
        }
        self.data.push(value);
    }
}
