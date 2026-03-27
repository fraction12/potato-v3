//! Modal overlays that float above the panel layout.

pub mod confirm;
pub mod help;
pub mod model_picker;
pub mod slash_menu;

use crossterm::event::KeyEvent;
use ratatui::{Frame, layout::Rect};

use crate::app::action::Action;

/// Trait implemented by all modal overlays.
pub trait Overlay {
    /// Render the overlay, centred over the given area.
    fn render(&self, frame: &mut Frame, area: Rect);

    /// Handle a key event while this overlay is active.
    fn handle_key(&mut self, key: KeyEvent) -> Action;

    /// Whether the overlay is currently visible.
    fn is_open(&self) -> bool;

    /// Close the overlay.
    fn close(&mut self);
}
