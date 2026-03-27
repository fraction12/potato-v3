//! Tool output panel — displays stdout/stderr from tool executions.

use crossterm::event::KeyEvent;
use ratatui::{Frame, layout::Rect};

use crate::app::action::Action;
use super::Panel;

/// Displays the output of the most recently executed tool.
#[derive(Debug, Default)]
pub struct ToolOutputPanel {
    /// Vertical scroll offset.
    pub scroll: usize,
}

impl Panel for ToolOutputPanel {
    fn render(&self, _frame: &mut Frame, _area: Rect) {}

    fn handle_key(&mut self, _key: KeyEvent) -> Action {
        Action::Noop
    }

    fn name(&self) -> &str {
        "Tool Output"
    }
}
