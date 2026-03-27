//! Chat panel — displays the conversation between user and AI agent.

use crossterm::event::KeyEvent;
use ratatui::{Frame, layout::Rect};

use crate::app::action::Action;
use super::Panel;

/// The primary chat panel showing conversation history and the input box.
#[derive(Debug, Default)]
pub struct ChatPanel {
    /// Vertical scroll offset.
    pub scroll: usize,
}

impl Panel for ChatPanel {
    fn render(&self, _frame: &mut Frame, _area: Rect) {}

    fn handle_key(&mut self, _key: KeyEvent) -> Action {
        Action::Noop
    }

    fn name(&self) -> &str {
        "Chat"
    }
}
