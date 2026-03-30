//! Help overlay — keyboard shortcut reference sheet.

use std::cell::Cell;

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph},
};

use super::{Overlay, OverlayAction};
use crate::ui::theme::{AMBER, BG, CHARCOAL, CREAM, SOIL, STONE};

// ── Keybind entry ─────────────────────────────────────────────────────────────

struct KeyEntry {
    keybind: &'static str,
    description: &'static str,
}

impl KeyEntry {
    const fn new(keybind: &'static str, description: &'static str) -> Self {
        Self {
            keybind,
            description,
        }
    }
}

// ── Section ───────────────────────────────────────────────────────────────────

struct Section {
    title: &'static str,
    entries: &'static [KeyEntry],
}

// ── Keybind data ──────────────────────────────────────────────────────────────

static GLOBAL_ENTRIES: &[KeyEntry] = &[
    KeyEntry::new("Ctrl+\\", "Quit"),
    KeyEntry::new("Ctrl+W", "Close active pane"),
    KeyEntry::new("Tab", "Next focus panel / cycle panes"),
    KeyEntry::new("Shift+Tab", "Previous focus panel"),
    KeyEntry::new("F1", "Toggle help"),
    KeyEntry::new("F2", "Agent picker"),
    KeyEntry::new("F3", "Session picker"),
    KeyEntry::new("F5", "Refresh git / OpenSpec"),
    KeyEntry::new("F6", "Focus terminal"),
];

static INPUT_ENTRIES: &[KeyEntry] = &[
    KeyEntry::new("Enter", "Broadcast to all agents"),
    KeyEntry::new("Esc", "Clear input"),
];

static TERMINAL_ENTRIES: &[KeyEntry] = &[
    KeyEntry::new("Tab", "Exit terminal focus (forward)"),
    KeyEntry::new("Ctrl+\\", "Quit (only intercept)"),
    KeyEntry::new("PgUp/PgDn", "Scroll terminal viewport"),
    KeyEntry::new("End", "Jump to bottom"),
    KeyEntry::new("*", "All other keys → agent PTY"),
];

static NAVIGATION_ENTRIES: &[KeyEntry] = &[
    KeyEntry::new("Tab", "Next focus panel"),
    KeyEntry::new("Shift+Tab", "Previous focus panel"),
];

static SECTIONS: &[Section] = &[
    Section {
        title: "Global",
        entries: GLOBAL_ENTRIES,
    },
    Section {
        title: "Input",
        entries: INPUT_ENTRIES,
    },
    Section {
        title: "Terminal",
        entries: TERMINAL_ENTRIES,
    },
    Section {
        title: "Navigation",
        entries: NAVIGATION_ENTRIES,
    },
];

// ── HelpOverlay ───────────────────────────────────────────────────────────────

/// Full-screen modal listing all keyboard shortcuts.
#[derive(Debug)]
pub struct HelpOverlay {
    /// Vertical scroll offset (in content rows).
    pub scroll: usize,
    /// Last rendered visible height (cached from render for key handling).
    visible_height: Cell<usize>,
}

impl Default for HelpOverlay {
    fn default() -> Self {
        Self {
            scroll: 0,
            visible_height: Cell::new(24),
        }
    }
}

impl HelpOverlay {
    /// Create a new help overlay scrolled to the top.
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
                let desc_span = Span::styled(entry.description, Style::default().fg(CREAM));
                lines.push(Line::from(vec![key_span, desc_span]));
            }

            // Blank separator between sections.
            lines.push(Line::from(""));
        }

        lines
    }

    /// Total scrollable content height.
    #[must_use]
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
        self.visible_height.set(visible);

        let content_area = if total > visible {
            // Reserve bottom row for scroll hint.
            let [content, hint_area] =
                Layout::vertical([Constraint::Fill(1), Constraint::Length(1)]).areas(inner);

            let hint = if self.scroll + visible < total {
                "↓ more"
            } else {
                "— end —"
            };
            let hint_line = Paragraph::new(hint).style(Style::default().fg(STONE));
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

        let para = Paragraph::new(sliced).style(Style::default().fg(CREAM).bg(BG));
        frame.render_widget(para, content_area);
    }

    fn handle_key(&mut self, key: KeyEvent) -> OverlayAction {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => OverlayAction::Close,
            KeyCode::Char('k') | KeyCode::Up => {
                self.scroll_up();
                OverlayAction::None
            }
            KeyCode::Char('j') | KeyCode::Down => {
                // Use a reasonable default visible height (24) when we don't
                // have the actual frame height at key-handling time.
                self.scroll_down(self.visible_height.get());
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
    fn test_help_question_mark_no_longer_closes() {
        let mut h = HelpOverlay::new();
        // ? is now a normal character, not a shortcut — should not close help.
        assert_eq!(h.handle_key(key(KeyCode::Char('?'))), OverlayAction::None);
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
    fn test_help_sections_include_input_and_terminal() {
        let has_input = SECTIONS.iter().any(|s| s.title == "Input");
        let has_terminal = SECTIONS.iter().any(|s| s.title == "Terminal");
        assert!(has_input, "SECTIONS should include 'Input'");
        assert!(has_terminal, "SECTIONS should include 'Terminal'");
    }

    #[test]
    fn test_help_global_entries_include_f2_f3() {
        let has_f2 = GLOBAL_ENTRIES.iter().any(|e| e.keybind == "F2");
        let has_f3 = GLOBAL_ENTRIES.iter().any(|e| e.keybind == "F3");
        assert!(has_f2, "GLOBAL_ENTRIES should include F2");
        assert!(has_f3, "GLOBAL_ENTRIES should include F3");
    }
}
