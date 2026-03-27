//! Model picker overlay — select which LLM model to use.

use crossterm::event::KeyEvent;
use ratatui::{Frame, layout::Rect};

use crate::app::action::Action;
use super::Overlay;

/// Modal listing available models; pressing Enter switches the active model.
#[derive(Debug, Default)]
pub struct ModelPicker {
    /// Whether the overlay is open.
    pub open: bool,
    /// List of available model names.
    pub models: Vec<String>,
    /// Currently highlighted model index.
    pub selected: usize,
}

impl Overlay for ModelPicker {
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
