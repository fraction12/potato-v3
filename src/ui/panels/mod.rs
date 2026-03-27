//! UI panels — each panel owns a region of the terminal.

pub mod agent_status;
pub mod chat;
pub mod file_preview;
pub mod sessions;
pub mod token_dash;
pub mod tool_output;

use crossterm::event::KeyEvent;
use ratatui::{Frame, layout::Rect};

use crate::app::action::Action;

/// Trait implemented by every panel in the layout.
pub trait Panel {
    /// Render the panel into the given area on the frame.
    fn render(&self, frame: &mut Frame, area: Rect);

    /// Handle a key event and return an optional action.
    fn handle_key(&mut self, key: KeyEvent) -> Action;

    /// Human-readable name of this panel (for debug / help overlay).
    fn name(&self) -> &str;
}
