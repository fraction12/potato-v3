//! File preview panel — syntax-highlighted view of a file on disk.

use crossterm::event::KeyEvent;
use ratatui::{Frame, layout::Rect};

use crate::app::state::AppState;
use super::{Panel, PanelAction, PanelId};

/// Shows a syntax-highlighted preview of the currently focused file.
#[derive(Debug, Default)]
pub struct FilePreviewPanel {
    /// Path of the file currently being previewed.
    pub current_path: Option<String>,
    /// Vertical scroll offset.
    pub scroll: usize,
    visible: bool,
}

impl FilePreviewPanel {
    pub fn new() -> Self {
        Self {
            current_path: None,
            scroll: 0,
            visible: true,
        }
    }
}

impl Panel for FilePreviewPanel {
    fn id(&self) -> PanelId {
        PanelId::FilePreview
    }

    fn title(&self) -> &str {
        "File Preview"
    }

    fn render(&self, _frame: &mut Frame, _area: Rect, _focused: bool, _state: &AppState) {}

    fn handle_key(&mut self, _key: KeyEvent, _state: &mut AppState) -> PanelAction {
        PanelAction::None
    }

    fn is_visible(&self) -> bool {
        self.visible
    }

    fn set_visible(&mut self, visible: bool) {
        self.visible = visible;
    }
}
