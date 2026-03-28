//! Token sparkline widget — mini time-series chart for token usage.

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::Style,
    text::Span,
    widgets::Widget,
};

use crate::ui::theme::AMBER;

// ── Bar characters ────────────────────────────────────────────────────────────

/// Braille-style bar characters ordered from lowest (1/8) to highest (8/8).
const BARS: &[char] = &['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

// ── TokenSparkline ────────────────────────────────────────────────────────────

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

    /// Push a new token count sample, evicting the oldest if at capacity.
    pub fn push(&mut self, value: u64) {
        if self.capacity > 0 && self.data.len() >= self.capacity {
            self.data.remove(0);
        }
        self.data.push(value);
    }

    /// Render the sparkline as a single-line string.
    ///
    /// Maps each data point to one of the 8 bar characters based on the
    /// overall min/max range of the dataset.  Empty data → empty string.
    pub fn render_str(&self) -> String {
        if self.data.is_empty() {
            return String::new();
        }

        let max = *self.data.iter().max().unwrap_or(&1);
        let min = *self.data.iter().min().unwrap_or(&0);

        self.data
            .iter()
            .map(|&v| bar_char(v, min, max))
            .collect()
    }
}

/// Map a value in `[min, max]` to one of the 8 bar characters.
fn bar_char(value: u64, min: u64, max: u64) -> char {
    if max == min {
        // All values equal — use the mid bar.
        return BARS[BARS.len() / 2];
    }

    let range = max - min;
    // Scale to [0, BARS.len()-1].
    let idx = ((value - min) as f64 / range as f64 * (BARS.len() - 1) as f64).round() as usize;
    BARS[idx.min(BARS.len() - 1)]
}

// ── ratatui Widget impl ───────────────────────────────────────────────────────

impl Widget for &TokenSparkline {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.height == 0 || area.width == 0 {
            return;
        }

        let sparkline_str = self.render_str();
        if sparkline_str.is_empty() {
            return;
        }

        // Truncate to fit the widget width (one char per cell).
        let row = area.top();
        let style = Style::default().fg(AMBER);

        let chars: Vec<char> = sparkline_str.chars().collect();
        let visible = chars.len().min(area.width as usize);

        for (i, ch) in chars.iter().take(visible).enumerate() {
            let col = area.left() + i as u16;
            if let Some(cell) = buf.cell_mut((col, row)) {
                cell.set_char(*ch);
                cell.set_style(style);
            }
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sparkline_empty_data_is_empty_string() {
        let s = TokenSparkline::new(10);
        assert_eq!(s.render_str(), "");
    }

    #[test]
    fn test_sparkline_single_value_uses_mid_bar() {
        let mut s = TokenSparkline::new(10);
        s.push(42);
        let result = s.render_str();
        // Single value: all equal → mid bar.
        assert_eq!(result.chars().count(), 1);
        let mid_bar = BARS[BARS.len() / 2];
        assert_eq!(result.chars().next().unwrap(), mid_bar);
    }

    #[test]
    fn test_sparkline_min_maps_to_first_bar() {
        let mut s = TokenSparkline::new(10);
        s.push(0);
        s.push(100);
        let chars: Vec<char> = s.render_str().chars().collect();
        // First value (0 = min) should map to the lowest bar.
        assert_eq!(chars[0], BARS[0]);
    }

    #[test]
    fn test_sparkline_max_maps_to_last_bar() {
        let mut s = TokenSparkline::new(10);
        s.push(0);
        s.push(100);
        let chars: Vec<char> = s.render_str().chars().collect();
        // Second value (100 = max) should map to the highest bar.
        assert_eq!(chars[1], BARS[BARS.len() - 1]);
    }

    #[test]
    fn test_sparkline_capacity_ring_buffer() {
        let mut s = TokenSparkline::new(3);
        s.push(1);
        s.push(2);
        s.push(3);
        s.push(4); // Should evict 1.
        assert_eq!(s.data, vec![2, 3, 4]);
        assert_eq!(s.data.len(), 3);
    }

    #[test]
    fn test_sparkline_push_within_capacity() {
        let mut s = TokenSparkline::new(5);
        s.push(10);
        s.push(20);
        assert_eq!(s.data.len(), 2);
    }

    #[test]
    fn test_sparkline_render_str_length_matches_data() {
        let mut s = TokenSparkline::new(10);
        for i in 0..8u64 {
            s.push(i * 10);
        }
        let result = s.render_str();
        assert_eq!(result.chars().count(), 8);
    }

    #[test]
    fn test_sparkline_ascending_uses_ascending_bars() {
        let mut s = TokenSparkline::new(8);
        // Push values 0..=7 mapped across 8 distinct levels.
        for i in 0..8u64 {
            s.push(i);
        }
        let chars: Vec<char> = s.render_str().chars().collect();
        // Each bar should be non-descending.
        for i in 1..chars.len() {
            let prev_idx = BARS.iter().position(|&b| b == chars[i-1]).unwrap_or(0);
            let curr_idx = BARS.iter().position(|&b| b == chars[i]).unwrap_or(0);
            assert!(curr_idx >= prev_idx, "bars should be non-descending");
        }
    }
}
