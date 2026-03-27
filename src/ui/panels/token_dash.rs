//! Token dashboard panel — live token usage metrics and sparkline.

use crossterm::event::KeyEvent;
use ratatui::{Frame, layout::Rect};

use crate::app::action::Action;
use super::Panel;

/// Compact strip showing prompt tokens, completion tokens, and cost estimate.
#[derive(Debug, Default)]
pub struct TokenDashPanel {
    /// History of token counts for sparkline rendering.
    pub history: Vec<u64>,
}

impl Panel for TokenDashPanel {
    fn render(&self, _frame: &mut Frame, _area: Rect) {}

    fn handle_key(&mut self, _key: KeyEvent) -> Action {
        Action::Noop
    }

    fn name(&self) -> &str {
        "Token Dashboard"
    }
}
