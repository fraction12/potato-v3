//! File preview panel — syntax-highlighted view of a file on disk.

use crossterm::event::KeyEvent;
use ratatui::{Frame, layout::Rect};

use crate::app::action::Action;
use super::Panel;

/// Shows a syntax-highlighted preview of the currently focused file.
#[derive(Debug, Default)]
pub struct FilePreviewPanel {
    /// Path of the file currently being previewed.
    pub current_path: Option<String>,
    /// Vertical scroll offset.
    pub scroll: usize,
}

impl Panel for FilePreviewPanel {
    fn render(&self, _frame: &mut Frame, _area: Rect) {}

    fn handle_key(&mut self, _key: KeyEvent) -> Action {
        Action::Noop
    }

    fn name(&self) -> &str {
        "File Preview"
    }
}
