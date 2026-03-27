//! Sessions panel — list of past and active conversation sessions.

use crossterm::event::KeyEvent;
use ratatui::{Frame, layout::Rect};

use crate::app::action::Action;
use super::Panel;

/// Sidebar panel listing all sessions; click/select to switch.
#[derive(Debug, Default)]
pub struct SessionsPanel {
    /// Index of the highlighted session.
    pub selected: usize,
}

impl Panel for SessionsPanel {
    fn render(&self, _frame: &mut Frame, _area: Rect) {}

    fn handle_key(&mut self, _key: KeyEvent) -> Action {
        Action::Noop
    }

    fn name(&self) -> &str {
        "Sessions"
    }
}
