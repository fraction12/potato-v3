//! Sessions panel — lists saved sessions for quick switching.
//!
//! Keys (when focused):
//!   - ↑/↓ / k/j — navigate the session list
//!   - Enter      — load the highlighted session
//!   - `n`        — create a new session
//!   - `d`        — delete the highlighted session

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, StatefulWidget},
};

use crate::app::state::AppState;
use crate::ui::theme::{AMBER, BG, CHARCOAL, CREAM, SOIL, SPROUT, STONE, TAN};

use super::{Panel, PanelAction, PanelId};

// ── Session entry ─────────────────────────────────────────────────────────────

/// A lightweight session summary shown in the panel.
#[derive(Debug, Clone)]
pub struct SessionEntry {
    pub id: String,
    pub title: String,
    /// Human-readable date string.
    pub date: String,
    pub message_count: usize,
    pub is_current: bool,
}

// ── SessionsPanel ─────────────────────────────────────────────────────────────

/// Sidebar panel listing all sessions.
#[derive(Debug, Default)]
pub struct SessionsPanel {
    /// Cached list of sessions.
    pub sessions: Vec<SessionEntry>,
    /// Index of the highlighted session.
    pub selected: usize,
    /// Whether this panel is visible.
    visible: bool,
}

impl SessionsPanel {
    /// Create an empty, visible sessions panel with no sessions loaded.
    pub fn new() -> Self {
        Self {
            sessions: Vec::new(),
            selected: 0,
            visible: true,
        }
    }

    /// Replace the session list (call after loading from the store).
    pub fn set_sessions(&mut self, entries: Vec<SessionEntry>) {
        self.sessions = entries;
        // Keep selection in bounds.
        if self.selected >= self.sessions.len() && !self.sessions.is_empty() {
            self.selected = self.sessions.len() - 1;
        }
    }

    /// Move selection up.
    pub fn select_prev(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        }
    }

    /// Move selection down.
    pub fn select_next(&mut self) {
        if !self.sessions.is_empty() && self.selected + 1 < self.sessions.len() {
            self.selected += 1;
        }
    }

    /// Return the id of the currently selected session, if any.
    pub fn selected_id(&self) -> Option<&str> {
        self.sessions.get(self.selected).map(|s| s.id.as_str())
    }
}

impl Panel for SessionsPanel {
    fn id(&self) -> PanelId {
        PanelId::Sessions
    }

    fn title(&self) -> &str {
        "Sessions"
    }

    fn render(&self, frame: &mut Frame, area: Rect, focused: bool, _state: &AppState) {
        let border_style = if focused {
            Style::default().fg(AMBER)
        } else {
            Style::default().fg(CHARCOAL)
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(border_style)
            .title(Span::styled(" Sessions ", Style::default().fg(TAN)))
            .style(Style::default().bg(BG));

        let inner = block.inner(area);
        frame.render_widget(block, area);

        if self.sessions.is_empty() {
            use ratatui::widgets::{Paragraph, Widget};
            let hint = Paragraph::new(Span::styled(
                " No sessions yet. Press `n` to create one.",
                Style::default().fg(STONE),
            ))
            .style(Style::default().bg(BG));
            hint.render(inner, frame.buffer_mut());
            return;
        }

        // Build list items.
        let items: Vec<ListItem<'static>> = self
            .sessions
            .iter()
            .enumerate()
            .map(|(idx, s)| {
                let is_selected = idx == self.selected;
                let is_current = s.is_current;

                let marker = if is_current { "● " } else { "  " };
                let marker_style = if is_current {
                    Style::default().fg(SPROUT)
                } else {
                    Style::default().fg(STONE)
                };

                let title_style = if is_selected {
                    Style::default()
                        .fg(CREAM)
                        .add_modifier(Modifier::BOLD)
                        .bg(CHARCOAL)
                } else {
                    Style::default().fg(TAN)
                };

                let meta_style = Style::default().fg(STONE);

                let line = Line::from(vec![
                    Span::styled(marker.to_string(), marker_style),
                    Span::styled(s.title.clone(), title_style),
                    Span::raw(" "),
                    Span::styled(
                        format!("[{}] {} msg", s.date, s.message_count),
                        meta_style,
                    ),
                ]);
                ListItem::new(line)
            })
            .collect();

        let mut list_state = ListState::default();
        list_state.select(Some(self.selected));

        let list = List::new(items).style(Style::default().bg(BG));
        StatefulWidget::render(list, inner, frame.buffer_mut(), &mut list_state);
    }

    fn handle_key(&mut self, key: KeyEvent, _state: &mut AppState) -> PanelAction {
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            return PanelAction::None;
        }
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                self.select_prev();
                PanelAction::None
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.select_next();
                PanelAction::None
            }
            KeyCode::Enter => {
                // Signal intent to load the session — main loop handles it.
                PanelAction::None
            }
            KeyCode::Char('n') => PanelAction::None, // new session
            KeyCode::Char('d') => PanelAction::None, // delete
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

    fn make_entries(n: usize) -> Vec<SessionEntry> {
        (0..n)
            .map(|i| SessionEntry {
                id: format!("id_{}", i),
                title: format!("Session {}", i),
                date: "2024-01-01".into(),
                message_count: i,
                is_current: i == 0,
            })
            .collect()
    }

    #[test]
    fn test_sessions_panel_navigation() {
        let mut panel = SessionsPanel::new();
        panel.set_sessions(make_entries(3));
        assert_eq!(panel.selected, 0);

        panel.select_next();
        assert_eq!(panel.selected, 1);

        panel.select_next();
        assert_eq!(panel.selected, 2);

        // Cannot go past last.
        panel.select_next();
        assert_eq!(panel.selected, 2);

        panel.select_prev();
        assert_eq!(panel.selected, 1);
    }

    #[test]
    fn test_sessions_panel_selected_id() {
        let mut panel = SessionsPanel::new();
        panel.set_sessions(make_entries(3));
        assert_eq!(panel.selected_id(), Some("id_0"));

        panel.select_next();
        assert_eq!(panel.selected_id(), Some("id_1"));
    }

    #[test]
    fn test_sessions_panel_bounds_on_set() {
        let mut panel = SessionsPanel::new();
        panel.selected = 10; // out of bounds
        panel.set_sessions(make_entries(3));
        assert_eq!(panel.selected, 2); // clamped to last
    }

    #[test]
    fn test_sessions_panel_empty_selected_id() {
        let panel = SessionsPanel::new();
        assert_eq!(panel.selected_id(), None);
    }

    #[test]
    fn test_sessions_panel_visibility() {
        let mut panel = SessionsPanel::new();
        assert!(panel.is_visible());
        panel.set_visible(false);
        assert!(!panel.is_visible());
    }
}
