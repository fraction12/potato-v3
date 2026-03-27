//! Confirmation dialog overlay — yes/no prompt for destructive actions.

use crossterm::event::KeyEvent;
use ratatui::{Frame, layout::Rect};

use crate::app::action::Action;
use super::Overlay;

/// Compact yes/no confirmation dialog.
#[derive(Debug, Default)]
pub struct ConfirmDialog {
    /// Whether the dialog is open.
    pub open: bool,
    /// The question to present to the user.
    pub message: String,
    /// Action to emit when confirmed.
    pub on_confirm: Option<Action>,
}

impl Overlay for ConfirmDialog {
    fn render(&self, _frame: &mut Frame, _area: Rect) {}

    fn handle_key(&mut self, _key: KeyEvent) -> Action {
        Action::Noop
    }

    fn is_open(&self) -> bool {
        self.open
    }

    fn close(&mut self) {
        self.open = false;
        self.on_confirm = None;
    }
}
