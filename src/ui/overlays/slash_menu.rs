//! Slash command menu overlay — fuzzy-searchable list of slash commands.

use crossterm::event::KeyEvent;
use ratatui::{Frame, layout::Rect};

use crate::app::action::Action;
use super::Overlay;

/// Modal showing available slash commands with fuzzy filtering.
#[derive(Debug, Default)]
pub struct SlashMenu {
    /// Whether the overlay is currently open.
    pub open: bool,
    /// Current filter query typed by the user.
    pub query: String,
    /// Index of the highlighted item.
    pub selected: usize,
}

impl Overlay for SlashMenu {
    fn render(&self, _frame: &mut Frame, _area: Rect) {}

    fn handle_key(&mut self, _key: KeyEvent) -> Action {
        Action::Noop
    }

    fn is_open(&self) -> bool {
        self.open
    }

    fn close(&mut self) {
        self.open = false;
        self.query.clear();
    }
}
