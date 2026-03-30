//! File preview panel — displays a file with line numbers and scrolling.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Widget},
};

use crate::app::state::AppState;
use crate::ui::theme::{AMBER, BG, CHARCOAL, CREAM, SOIL, STONE};
use super::{Panel, PanelAction, PanelId};

// ── Constants ─────────────────────────────────────────────────────────────────

/// Maximum number of lines to load for large files.
const MAX_LINES: usize = 500;

/// Bytes to inspect when detecting binary files.
const BINARY_CHECK_BYTES: usize = 512;

// ── FilePreviewPanel ──────────────────────────────────────────────────────────

/// Shows a file with line numbers, scrolling, and basic language detection.
#[derive(Debug, Default)]
pub struct FilePreviewPanel {
    /// Path of the file currently being previewed.
    pub current_path: Option<String>,
    /// Vertical scroll offset (0-based line index).
    pub scroll: usize,
    /// Loaded file lines (after truncation if necessary).
    lines: Vec<String>,
    /// Whether the file was truncated (exceeded MAX_LINES).
    truncated: bool,
    /// Whether the file appears to be binary.
    binary: bool,
    /// Detected language extension (e.g. "rs", "py").
    pub language: Option<String>,
    /// Whether this panel is visible.
    visible: bool,
}

impl FilePreviewPanel {
    /// Create an empty, visible file preview panel with no file loaded.
    pub fn new() -> Self {
        Self {
            visible: true,
            ..Default::default()
        }
    }

    /// Load a file from disk into the panel.
    ///
    /// Detects binary content, truncates at MAX_LINES, and detects language.
    pub fn load_file(&mut self, path: impl Into<String>) {
        let path = path.into();

        // Detect language from extension.
        self.language = std::path::Path::new(&path)
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.to_lowercase());

        // Read file.
        match std::fs::read(&path) {
            Ok(bytes) => {
                // Binary detection: check for null bytes in the first BINARY_CHECK_BYTES.
                let check_len = bytes.len().min(BINARY_CHECK_BYTES);
                self.binary = bytes[..check_len].contains(&0u8);

                if self.binary {
                    self.lines = vec!["[binary file — cannot display]".to_string()];
                    self.truncated = false;
                } else {
                    let content = String::from_utf8_lossy(&bytes).to_string();
                    let all_lines: Vec<String> = content.lines().map(|l| l.to_string()).collect();
                    self.truncated = all_lines.len() > MAX_LINES;
                    self.lines = all_lines.into_iter().take(MAX_LINES).collect();
                }
            }
            Err(e) => {
                self.lines = vec![format!("[error reading file: {}]", e)];
                self.truncated = false;
                self.binary = false;
            }
        }

        self.current_path = Some(path);
        self.scroll = 0;
    }

    /// Reload the current file (e.g. after an external change).
    pub fn reload(&mut self) {
        if let Some(path) = self.current_path.clone() {
            self.load_file(path);
        }
    }

    /// Total number of content lines loaded.
    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    /// Scroll down by `delta` lines, clamped to content.
    fn scroll_down(&mut self, delta: usize, visible_height: usize) {
        let max_scroll = self.lines.len().saturating_sub(visible_height);
        self.scroll = (self.scroll + delta).min(max_scroll);
    }

    /// Scroll up by `delta` lines, clamped to 0.
    fn scroll_up(&mut self, delta: usize) {
        self.scroll = self.scroll.saturating_sub(delta);
    }

    /// Jump to end.
    fn scroll_to_bottom(&mut self, visible_height: usize) {
        self.scroll = self.lines.len().saturating_sub(visible_height);
    }
}

impl Panel for FilePreviewPanel {
    fn id(&self) -> PanelId {
        PanelId::FilePreview
    }

    fn title(&self) -> &str {
        // Returns a static str — dynamic title shown via render block title.
        "File Preview"
    }

    fn render(&self, frame: &mut Frame, area: Rect, focused: bool, _state: &AppState) {
        let border_style = if focused {
            Style::default().fg(AMBER)
        } else {
            Style::default().fg(CHARCOAL)
        };

        // Build dynamic title from path.
        let title = match &self.current_path {
            Some(p) => {
                let name = std::path::Path::new(p)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or(p.as_str());
                format!(" {} ", name)
            }
            None => " File Preview ".to_string(),
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(border_style)
            .title(title)
            .title_style(Style::default().fg(if focused { AMBER } else { CHARCOAL }).add_modifier(Modifier::BOLD))
            .style(Style::default().bg(BG));

        let inner = block.inner(area);
        frame.render_widget(block, area);

        if inner.height == 0 || inner.width == 0 {
            return;
        }

        let visible_height = inner.height as usize;

        // Determine the line number gutter width.
        let total_lines = self.lines.len();
        let gutter_width = if total_lines == 0 {
            3
        } else {
            format!("{}", total_lines).len() + 1
        };

        // Build visible lines.
        let mut rendered_lines: Vec<Line<'static>> = Vec::new();

        for (i, line_content) in self.lines.iter().enumerate().skip(self.scroll).take(visible_height) {
            let line_num = i + 1;
            let num_str = format!("{:>width$} ", line_num, width = gutter_width - 1);

            let num_span = Span::styled(
                num_str,
                Style::default().fg(STONE),
            );
            let content_span = Span::styled(
                line_content.clone(),
                Style::default().fg(CREAM),
            );

            rendered_lines.push(Line::from(vec![num_span, content_span]));
        }

        // If truncated and we've scrolled near the bottom, show truncation notice.
        if self.truncated && rendered_lines.len() < visible_height {
            rendered_lines.push(Line::from(Span::styled(
                format!("  [truncated — showing first {} lines]", MAX_LINES),
                Style::default().fg(AMBER).add_modifier(Modifier::ITALIC),
            )));
        }

        // Empty state.
        if rendered_lines.is_empty() {
            rendered_lines.push(Line::from(Span::styled(
                "No file loaded. Use Ctrl+P to open a file.",
                Style::default().fg(STONE),
            )));
        }

        let para = Paragraph::new(rendered_lines)
            .style(Style::default().bg(BG));
        frame.render_widget(para, inner);
    }

    fn handle_key(&mut self, key: KeyEvent, _state: &mut AppState) -> PanelAction {
        // Use a reasonable default for visible height when not known.
        let visible_height: usize = 30;

        match key.code {
            KeyCode::Char('j') | KeyCode::Down => {
                self.scroll_down(1, visible_height);
                PanelAction::None
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.scroll_up(1);
                PanelAction::None
            }
            KeyCode::PageDown => {
                self.scroll_down(visible_height / 2, visible_height);
                PanelAction::None
            }
            KeyCode::PageUp => {
                self.scroll_up(visible_height / 2);
                PanelAction::None
            }
            KeyCode::Char('G') => {
                self.scroll_to_bottom(visible_height);
                PanelAction::None
            }
            KeyCode::Char('g') => {
                self.scroll = 0;
                PanelAction::None
            }
            KeyCode::Char('r') => {
                self.reload();
                PanelAction::None
            }
            _ => PanelAction::None,
        }
    }

    fn is_visible(&self) -> bool {
        self.visible
    }

    fn set_visible(&mut self, visible: bool) {
        self.visible = visible;
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// Write content to a temp path and return that path.
    fn make_temp_file(name: &str, content: &[u8]) -> PathBuf {
        let path = std::env::temp_dir().join(format!("potato_test_{}", name));
        std::fs::write(&path, content).expect("write temp file");
        path
    }

    #[test]
    fn test_file_preview_id() {
        let panel = FilePreviewPanel::new();
        assert_eq!(panel.id(), PanelId::FilePreview);
    }

    #[test]
    fn test_load_file_reads_lines() {
        let path = make_temp_file("preview_basic.txt", b"line1\nline2\nline3\n");
        let mut panel = FilePreviewPanel::new();
        panel.load_file(path.to_str().unwrap());
        assert_eq!(panel.line_count(), 3);
        assert!(!panel.binary);
        assert!(!panel.truncated);
    }

    #[test]
    fn test_load_file_language_detection() {
        let path = make_temp_file("preview_lang.rs", b"fn main() {}");
        let mut panel = FilePreviewPanel::new();
        panel.load_file(path.to_str().unwrap());
        assert_eq!(panel.language.as_deref(), Some("rs"));
    }

    #[test]
    fn test_load_file_binary_detection() {
        let path = make_temp_file("preview_binary.bin", b"hello\x00world");
        let mut panel = FilePreviewPanel::new();
        panel.load_file(path.to_str().unwrap());
        assert!(panel.binary);
    }

    #[test]
    fn test_load_nonexistent_file_shows_error() {
        let mut panel = FilePreviewPanel::new();
        panel.load_file("/nonexistent/path/that/does/not/exist_potato.txt");
        assert_eq!(panel.line_count(), 1);
        assert!(panel.lines[0].contains("error"));
    }

    #[test]
    fn test_scroll_down_and_up() {
        let content: String = (0..50).map(|i| format!("line {}\n", i)).collect();
        let path = make_temp_file("preview_scroll.txt", content.as_bytes());
        let mut panel = FilePreviewPanel::new();
        panel.load_file(path.to_str().unwrap());
        assert_eq!(panel.scroll, 0);

        panel.scroll_down(5, 10);
        assert_eq!(panel.scroll, 5);

        panel.scroll_up(3);
        assert_eq!(panel.scroll, 2);
    }

    #[test]
    fn test_scroll_clamped_at_bottom() {
        let path = make_temp_file("preview_clamp.txt", b"a\nb\nc\n");
        let mut panel = FilePreviewPanel::new();
        panel.load_file(path.to_str().unwrap());
        // 3 lines, visible_height=10 → max_scroll = 3.saturating_sub(10) = 0
        panel.scroll_down(100, 10);
        assert_eq!(panel.scroll, 0);
    }

    #[test]
    fn test_reload_re_reads_file() {
        let path = make_temp_file("preview_reload.txt", b"original\n");
        let mut panel = FilePreviewPanel::new();
        panel.load_file(path.to_str().unwrap());
        assert_eq!(panel.line_count(), 1);

        // Overwrite the file.
        std::fs::write(&path, "line1\nline2\n").expect("overwrite");
        panel.reload();
        assert_eq!(panel.line_count(), 2);
    }

    #[test]
    fn test_visibility_toggle() {
        let mut panel = FilePreviewPanel::new();
        assert!(panel.is_visible());
        panel.set_visible(false);
        assert!(!panel.is_visible());
    }
}
