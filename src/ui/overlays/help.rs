//! Help overlay — keyboard shortcut reference sheet.

use crossterm::event::KeyEvent;
use ratatui::{Frame, layout::Rect};

use crate::app::action::Action;
use super::Overlay;

/// Full-screen modal listing all keyboard shortcuts.
#[derive(Debug, Default)]
pub struct HelpOverlay {
    /// Whether the overlay is open.
    pub open: bool,
    /// Vertical scroll offset.
    pub scroll: usize,
}

impl Overlay for HelpOverlay {
    fn render(&self, _frame: &mut Frame, _area: Rect) {}

    fn handle_key(&mut self, _key: KeyEvent) -> Action {
        Action::Noop
    }

    fn is_open(&self) -> bool {
        self.open
    }

    fn close(&mut self) {
        self.open = false;
    }
}
