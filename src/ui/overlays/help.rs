//! Help overlay — keyboard shortcut reference sheet.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph},
};

use crate::ui::theme::{AMBER, BG, CHARCOAL, CREAM, SOIL, STONE};
use super::{Overlay, OverlayAction};

// ── Keybind entry ─────────────────────────────────────────────────────────────

struct KeyEntry {
    keybind: &'static str,
    description: &'static str,
}

impl KeyEntry {
    const fn new(keybind: &'static str, description: &'static str) -> Self {
        Self { keybind, description }
    }
}

// ── Section ───────────────────────────────────────────────────────────────────

struct Section {
    title: &'static str,
    entries: &'static [KeyEntry],
}

// ── Keybind data ──────────────────────────────────────────────────────────────

static GLOBAL_ENTRIES: &[KeyEntry] = &[
    KeyEntry::new("Ctrl+\\",     "Quit"),
    KeyEntry::new("Ctrl+W",      "Close active pane"),
    KeyEntry::new("Ctrl+] / [",  "Next / prev pane"),
    KeyEntry::new("Tab",         "Next focus panel"),
    KeyEntry::new("Shift+Tab",   "Previous focus panel"),
    KeyEntry::new("?",           "Toggle help"),
];

static INPUT_ENTRIES: &[KeyEntry] = &[
    KeyEntry::new("Enter",  "Send to Claude / Execute command"),
    KeyEntry::new("/",      "Start command mode"),
    KeyEntry::new("Esc",    "Clear input"),
];

static TERMINAL_ENTRIES: &[KeyEntry] = &[
    KeyEntry::new("Ctrl+J",     "Focus terminal"),
    KeyEntry::new("Ctrl+Q",     "Exit terminal focus"),
    KeyEntry::new("Esc",        "Passes through to agent"),
    KeyEntry::new("PgUp/PgDn",  "Scroll terminal"),
    KeyEntry::new("End",        "Jump to bottom"),
];

static NAVIGATION_ENTRIES: &[KeyEntry] = &[
    KeyEntry::new("Tab",        "Next focus panel"),
    KeyEntry::new("Shift+Tab",  "Previous focus panel"),
];

static COMMAND_ENTRIES: &[KeyEntry] = &[
    KeyEntry::new("/new",          "New Claude session"),
    KeyEntry::new("/sessions",     "Session picker"),
    KeyEntry::new("/export",       "Export session"),
    KeyEntry::new("/help",         "Show this help"),
    KeyEntry::new("/role <name>",  "Set pane role"),
    KeyEntry::new("/agent",        "Agent info"),
];

static SECTIONS: &[Section] = &[
    Section { title: "Global",     entries: GLOBAL_ENTRIES },
    Section { title: "Input",      entries: INPUT_ENTRIES },
    Section { title: "Terminal",   entries: TERMINAL_ENTRIES },
    Section { title: "Navigation", entries: NAVIGATION_ENTRIES },
    Section { title: "Commands",   entries: COMMAND_ENTRIES },
];

// ── HelpOverlay ───────────────────────────────────────────────────────────────

/// Full-screen modal listing all keyboard shortcuts.
#[derive(Debug, Default)]
pub struct HelpOverlay {
    /// Vertical scroll offset (in content rows).
    pub scroll: usize,
}

impl HelpOverlay {
    pub fn new() -> Self {
        Self::default()
    }

    /// Build the flat list of rendered lines (section headers + keybind rows).
    fn build_lines(&self) -> Vec<Line<'static>> {
        let mut lines: Vec<Line<'static>> = Vec::new();

        for section in SECTIONS {
            // Section header.
            lines.push(Line::from(Span::styled(
                section.title,
                Style::default().fg(AMBER).add_modifier(Modifier::BOLD),
            )));

            for entry in section.entries {
                let key_span = Span::styled(
                    format!("  {:<16}", entry.keybind),
                    Style::default().fg(AMBER),
                );
                let desc_span = Span::styled(
                    entry.description,
                    Style::default().fg(CREAM),
                );
                lines.push(Line::from(vec![key_span, desc_span]));
            }

            // Blank separator between sections.
            lines.push(Line::from(""));
        }

        lines
    }

    /// Total scrollable content height.
    pub fn content_height(&self) -> usize {
        self.build_lines().len()
    }

    fn scroll_up(&mut self) {
        self.scroll = self.scroll.saturating_sub(1);
    }

    fn scroll_down(&mut self, visible_height: usize) {
        let max = self.content_height().saturating_sub(visible_height);
        if self.scroll < max {
            self.scroll += 1;
        }
    }
}

impl Overlay for HelpOverlay {
    fn title(&self) -> &str {
        "Keyboard Shortcuts"
    }

    fn render(&self, frame: &mut Frame, area: Rect) {
        // Centered box, 70% of terminal width and 80% of terminal height.
        let width = (area.width * 7 / 10).max(50).min(area.width);
        let height = (area.height * 8 / 10).max(10).min(area.height);

        let x = area.left() + area.width.saturating_sub(width) / 2;
        let y = area.top() + area.height.saturating_sub(height) / 2;

        let overlay_area = Rect::new(x, y, width, height).intersection(area);

        frame.render_widget(Clear, overlay_area);

        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(AMBER))
            .title(format!(" {} ", self.title()))
            .title_style(Style::default().fg(AMBER).add_modifier(Modifier::BOLD))
            .style(Style::default().bg(BG));

        let inner = block.inner(overlay_area);
        frame.render_widget(block, overlay_area);

        // Add a scroll indicator at bottom if there is overflow.
        let lines = self.build_lines();
        let total = lines.len();
        let visible = inner.height as usize;

        let content_area = if total > visible {
            // Reserve bottom row for scroll hint.
            let [content, hint_area] = Layout::vertical([
                Constraint::Fill(1),
                Constraint::Length(1),
            ]).areas(inner);

            let hint = if self.scroll + visible < total {
                "↓ more"
            } else {
                "— end —"
            };
            let hint_line = Paragraph::new(hint)
                .style(Style::default().fg(STONE));
            frame.render_widget(hint_line, hint_area);

            content
        } else {
            inner
        };

        let sliced: Vec<Line<'static>> = lines
            .into_iter()
            .skip(self.scroll)
            .take(content_area.height as usize)
            .collect();

        let para = Paragraph::new(sliced)
            .style(Style::default().fg(CREAM).bg(BG));
        frame.render_widget(para, content_area);
    }

    fn handle_key(&mut self, key: KeyEvent) -> OverlayAction {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('?') => OverlayAction::Close,
            KeyCode::Char('k') | KeyCode::Up => {
                self.scroll_up();
                OverlayAction::None
            }
            KeyCode::Char('j') | KeyCode::Down => {
                // Use a reasonable default visible height (24) when we don't
                // have the actual frame height at key-handling time.
                self.scroll_down(24);
                OverlayAction::None
            }
            _ => OverlayAction::None,
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyModifiers;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn test_help_title() {
        let h = HelpOverlay::new();
        assert_eq!(h.title(), "Keyboard Shortcuts");
    }

    #[test]
    fn test_help_esc_closes() {
        let mut h = HelpOverlay::new();
        assert_eq!(h.handle_key(key(KeyCode::Esc)), OverlayAction::Close);
    }

    #[test]
    fn test_help_question_mark_closes() {
        let mut h = HelpOverlay::new();
        assert_eq!(h.handle_key(key(KeyCode::Char('?'))), OverlayAction::Close);
    }

    #[test]
    fn test_help_content_height_nonzero() {
        let h = HelpOverlay::new();
        assert!(h.content_height() > 0);
    }

    #[test]
    fn test_help_scroll_up_clamps_at_zero() {
        let mut h = HelpOverlay::new();
        h.scroll = 0;
        h.scroll_up();
        assert_eq!(h.scroll, 0);
    }

    #[test]
    fn test_help_scroll_down_then_up() {
        let mut h = HelpOverlay::new();
        h.scroll_down(1); // content_height > 1, so scroll should advance
        assert!(h.scroll >= 1 || h.content_height() <= 1);
        h.scroll_up();
        assert_eq!(h.scroll, 0);
    }

    #[test]
    fn test_help_build_lines_contains_sections() {
        let h = HelpOverlay::new();
        let lines = h.build_lines();
        // Should contain at least one line per section + entries.
        assert!(lines.len() >= SECTIONS.len());
    }

    #[test]
    fn test_help_sections_include_commands() {
        // Verify the Commands section is present in SECTIONS.
        let has_commands = SECTIONS.iter().any(|s| s.title == "Commands");
        assert!(has_commands, "SECTIONS should include a 'Commands' section");
    }

    #[test]
    fn test_help_sections_include_input_and_terminal() {
        let has_input = SECTIONS.iter().any(|s| s.title == "Input");
        let has_terminal = SECTIONS.iter().any(|s| s.title == "Terminal");
        assert!(has_input, "SECTIONS should include 'Input'");
        assert!(has_terminal, "SECTIONS should include 'Terminal'");
    }

    #[test]
    fn test_help_commands_section_has_role_entry() {
        // /role <name> should appear in the Commands section entries.
        let commands_section = SECTIONS.iter().find(|s| s.title == "Commands").unwrap();
        let has_role = commands_section.entries.iter().any(|e| e.keybind.contains("/role"));
        assert!(has_role, "Commands section should have a /role entry");
    }
}
