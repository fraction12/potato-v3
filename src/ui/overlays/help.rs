//! Help overlay — keyboard shortcut reference sheet.

use crossterm::event::KeyEvent;
use ratatui::{Frame, layout::Rect};

use super::{Overlay, OverlayAction};

/// Full-screen modal listing all keyboard shortcuts.
#[derive(Debug, Default)]
pub struct HelpOverlay {
    /// Vertical scroll offset.
    pub scroll: usize,
}

impl Overlay for HelpOverlay {
    fn title(&self) -> &str {
        "Help"
    }

    fn render(&self, _frame: &mut Frame, _area: Rect) {}

    fn handle_key(&mut self, _key: KeyEvent) -> OverlayAction {
        OverlayAction::Close
    }
}
