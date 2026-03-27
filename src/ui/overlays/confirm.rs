//! Confirmation dialog overlay — yes/no prompt for destructive actions.

use crossterm::event::KeyEvent;
use ratatui::{Frame, layout::Rect};

use super::{Overlay, OverlayAction};

/// Compact yes/no confirmation dialog.
#[derive(Debug, Default)]
pub struct ConfirmDialog {
    /// Whether the dialog is open.
    pub open: bool,
    /// The question to present to the user.
    pub message: String,
}

impl Overlay for ConfirmDialog {
    fn title(&self) -> &str {
        "Confirm"
    }

    fn render(&self, _frame: &mut Frame, _area: Rect) {}

    fn handle_key(&mut self, _key: KeyEvent) -> OverlayAction {
        OverlayAction::Close
    }
}
