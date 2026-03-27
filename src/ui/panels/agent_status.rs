//! Agent status panel — bottom bar showing current agent phase and model.

use crossterm::event::KeyEvent;
use ratatui::{Frame, layout::Rect};

use crate::app::action::Action;
use super::Panel;

/// Single-line status bar at the bottom of the screen.
#[derive(Debug, Default)]
pub struct AgentStatusPanel;

impl Panel for AgentStatusPanel {
    fn render(&self, _frame: &mut Frame, _area: Rect) {}

    fn handle_key(&mut self, _key: KeyEvent) -> Action {
        Action::Noop
    }

    fn name(&self) -> &str {
        "Agent Status"
    }
}
