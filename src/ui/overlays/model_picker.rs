//! Model picker overlay — select which LLM model to use.

use crossterm::event::KeyEvent;
use ratatui::{Frame, layout::Rect};

use super::{Overlay, OverlayAction};

/// Modal listing available models; pressing Enter switches the active model.
#[derive(Debug, Default)]
pub struct ModelPicker {
    /// List of available model names.
    pub models: Vec<String>,
    /// Currently highlighted model index.
    pub selected: usize,
}

impl Overlay for ModelPicker {
    fn title(&self) -> &str {
        "Model Picker"
    }

    fn render(&self, _frame: &mut Frame, _area: Rect) {}

    fn handle_key(&mut self, _key: KeyEvent) -> OverlayAction {
        OverlayAction::Close
    }
}
