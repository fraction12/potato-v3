//! Slash command menu overlay — fuzzy-searchable list of slash commands.
//!
//! Triggered when the user types `/` in the input box. Filters commands as the
//! user continues typing. Arrow keys navigate; Enter selects; Esc cancels.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use nucleo_matcher::{
    Config, Matcher,
    pattern::{AtomKind, CaseMatching, Normalization, Pattern},
};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph, Widget},
};

use crate::ui::theme::{AMBER, BG, CHARCOAL, CREAM, SOIL, STONE};
use super::{Overlay, OverlayAction};

// ── SlashCommand ──────────────────────────────────────────────────────────────

/// A single slash command entry in the menu.
#[derive(Debug, Clone)]
pub struct SlashCommand {
    /// Canonical name without the leading slash (e.g. `"help"`).
    pub name: &'static str,
    /// One-line description shown next to the name.
    pub description: &'static str,
    /// Optional short alias (e.g. `"h"` for `help`).
    pub alias: Option<&'static str>,
}

impl SlashCommand {
    const fn new(name: &'static str, description: &'static str) -> Self {
        Self { name, description, alias: None }
    }

    const fn with_alias(name: &'static str, description: &'static str, alias: &'static str) -> Self {
        Self { name, description, alias: Some(alias) }
    }

    /// Returns the string used for fuzzy matching.
    pub fn match_text(&self) -> String {
        format!("/{}", self.name)
    }
}

// ── Built-in commands ─────────────────────────────────────────────────────────

static BUILTIN_COMMANDS: &[SlashCommand] = &[
    SlashCommand::with_alias("help",   "Show keyboard shortcuts",   "h"),
    SlashCommand::with_alias("model",  "Switch LLM model",          "m"),
    SlashCommand::new       ("new",    "Start a new session"),
    SlashCommand::new       ("load",   "Load a saved session"),
    SlashCommand::new       ("export", "Export conversation"),
    SlashCommand::with_alias("clear",  "Clear conversation history", "c"),
    SlashCommand::with_alias("quit",   "Quit Potato",                "q"),
];

// ── Shared filtering ─────────────────────────────────────────────────────────

/// Filter `BUILTIN_COMMANDS` by `query` using fuzzy matching (name + alias).
///
/// Returns commands sorted by descending match score. Empty query returns all.
fn filter_commands<'a>(query: &str, matcher: &mut Matcher) -> Vec<&'a SlashCommand> {
    if query.is_empty() {
        return BUILTIN_COMMANDS.iter().collect();
    }

    let pattern = Pattern::new(
        query,
        CaseMatching::Ignore,
        Normalization::Smart,
        AtomKind::Fuzzy,
    );

    let mut results: Vec<(&SlashCommand, u32)> = BUILTIN_COMMANDS
        .iter()
        .filter_map(|cmd| {
            // Try matching against "/name"
            let text = cmd.match_text();
            let score = pattern.score(
                nucleo_matcher::Utf32Str::new(&text, &mut Vec::new()),
                matcher,
            );
            // Also try alias match
            let alias_score = cmd.alias.and_then(|a| {
                let alias_text = format!("/{}", a);
                pattern.score(
                    nucleo_matcher::Utf32Str::new(&alias_text, &mut Vec::new()),
                    matcher,
                )
            });

            let best = match (score, alias_score) {
                (Some(s), Some(a)) => Some(s.max(a)),
                (Some(s), None) => Some(s),
                (None, Some(a)) => Some(a),
                (None, None) => None,
            };

            best.map(|s| (cmd, s as u32))
        })
        .collect();

    results.sort_by(|a, b| b.1.cmp(&a.1));
    results.into_iter().map(|(cmd, _)| cmd).collect()
}

// ── SlashMenu ─────────────────────────────────────────────────────────────────

/// Fuzzy-searchable slash command picker.
///
/// Appears floating above the input area when the user types `/`.
#[derive(Debug)]
pub struct SlashMenu {
    /// Current filter string (the text after `/`).
    pub query: String,
    /// Currently highlighted row index within the *filtered* list.
    pub selected: usize,
    /// Nucleo matcher (reused across key strokes).
    matcher: Matcher,
}

impl Default for SlashMenu {
    fn default() -> Self {
        Self::new()
    }
}

impl SlashMenu {
    /// Create a new slash menu with an empty query.
    pub fn new() -> Self {
        Self {
            query: String::new(),
            selected: 0,
            matcher: Matcher::new(Config::DEFAULT),
        }
    }

    /// Reset the menu state (query cleared, selection at top).
    pub fn reset(&mut self) {
        self.query.clear();
        self.selected = 0;
    }

    /// Return the list of commands that match the current query.
    pub fn filtered(&mut self) -> Vec<&SlashCommand> {
        filter_commands(&self.query, &mut self.matcher)
    }

    /// Move selection up (wraps).
    pub fn select_up(&mut self) {
        let count = self.visible_count();
        if count == 0 {
            return;
        }
        if self.selected == 0 {
            self.selected = count - 1;
        } else {
            self.selected -= 1;
        }
    }

    /// Move selection down (wraps).
    pub fn select_down(&mut self) {
        let count = self.visible_count();
        if count == 0 {
            return;
        }
        self.selected = (self.selected + 1) % count;
    }

    /// Clamp selection to valid range after filter changes.
    fn clamp_selection(&mut self) {
        let count = self.visible_count();
        if count == 0 {
            self.selected = 0;
        } else if self.selected >= count {
            self.selected = count - 1;
        }
    }

    fn visible_count(&mut self) -> usize {
        self.filtered().len()
    }
}

impl Overlay for SlashMenu {
    fn title(&self) -> &str {
        "Commands"
    }

    fn render(&self, frame: &mut Frame, area: Rect) {
        // Use a local matcher since render() takes &self (not &mut self).
        let mut matcher = Matcher::new(Config::DEFAULT);
        let commands = filter_commands(&self.query, &mut matcher);

        if commands.is_empty() {
            return;
        }

        // Overlay floats above the input area, left-anchored.
        let max_rows = commands.len().min(8) as u16;
        let height = max_rows + 2; // border
        let width = 50_u16.min(area.width);

        // Position above the bottom of the provided area.
        let y = area.bottom().saturating_sub(height + 3); // above input
        let x = area.left() + 1;

        let overlay_area = Rect::new(x, y, width, height).intersection(area);

        // Clear background behind the overlay.
        frame.render_widget(Clear, overlay_area);

        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(AMBER))
            .style(Style::default().bg(CHARCOAL));

        let inner = block.inner(overlay_area);
        frame.render_widget(block, overlay_area);

        // Row height = 1; render each command.
        let row_height = 1_u16;
        let mut y_off = 0_u16;
        for (i, cmd) in commands.iter().enumerate() {
            if y_off >= inner.height {
                break;
            }
            let row_area = Rect::new(inner.x, inner.y + y_off, inner.width, row_height);
            let is_selected = i == self.selected;

            let bg = if is_selected { CHARCOAL } else { BG };
            let name_style = if is_selected {
                Style::default().fg(CREAM).bg(bg).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(STONE).bg(bg)
            };
            let desc_style = Style::default().fg(STONE).bg(bg);

            let name_text = format!("/{}", cmd.name);
            let mut spans = vec![
                Span::styled(format!("{:<12}", name_text), name_style),
                Span::styled(format!(" {}", cmd.description), desc_style),
            ];
            if is_selected {
                // Pad to right edge and show "←" indicator
                spans.push(Span::styled(" ←", Style::default().fg(AMBER).bg(bg)));
            }

            let line = Line::from(spans);
            Paragraph::new(line).render(row_area, frame.buffer_mut());

            y_off += row_height;
        }
    }

    fn handle_key(&mut self, key: KeyEvent) -> OverlayAction {
        match key.code {
            KeyCode::Esc => OverlayAction::Close,
            KeyCode::Enter => {
                // Collect filtered names first to avoid borrow conflicts.
                let names: Vec<String> = self
                    .filtered()
                    .into_iter()
                    .map(|c| format!("/{}", c.name))
                    .collect();
                if let Some(name) = names.get(self.selected).cloned() {
                    self.reset();
                    OverlayAction::Select(name)
                } else {
                    OverlayAction::Close
                }
            }
            KeyCode::Up | KeyCode::Char('k') if key.modifiers == KeyModifiers::NONE => {
                self.select_up();
                OverlayAction::None
            }
            KeyCode::Down | KeyCode::Char('j') if key.modifiers == KeyModifiers::NONE => {
                self.select_down();
                OverlayAction::None
            }
            KeyCode::Backspace => {
                self.query.pop();
                self.clamp_selection();
                OverlayAction::None
            }
            KeyCode::Char(c) if key.modifiers == KeyModifiers::NONE || key.modifiers == KeyModifiers::SHIFT => {
                self.query.push(c);
                self.clamp_selection();
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

    fn enter_key() -> KeyEvent {
        KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)
    }
    fn up_key() -> KeyEvent {
        KeyEvent::new(KeyCode::Up, KeyModifiers::NONE)
    }
    fn down_key() -> KeyEvent {
        KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)
    }
    fn char_key(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
    }

    // ── test_slash_menu_filters_commands ─────────────────────────────────────

    #[test]
    fn test_slash_menu_filters_commands() {
        let mut menu = SlashMenu::new();
        // With empty query, all commands are returned.
        assert_eq!(menu.filtered().len(), BUILTIN_COMMANDS.len());

        // Typing "help" should match the /help command.
        menu.query = "help".to_string();
        let results = menu.filtered();
        assert!(!results.is_empty(), "filtering by 'help' should return results");
        assert!(
            results.iter().any(|c| c.name == "help"),
            "/help should appear in filtered results"
        );

        // Typing something that matches nothing returns empty.
        menu.query = "zzzzzzz".to_string();
        let results = menu.filtered();
        assert!(results.is_empty(), "garbage query should return no results");
    }

    // ── test_slash_menu_fuzzy_match ───────────────────────────────────────────

    #[test]
    fn test_slash_menu_fuzzy_match() {
        let mut menu = SlashMenu::new();
        // "md" should fuzzy-match "/model".
        menu.query = "md".to_string();
        let results = menu.filtered();
        assert!(
            results.iter().any(|c| c.name == "model"),
            "'md' should fuzzy-match /model, got: {:?}",
            results.iter().map(|c| c.name).collect::<Vec<_>>()
        );

        // "cl" should fuzzy-match "/clear".
        menu.query = "cl".to_string();
        let results = menu.filtered();
        assert!(
            results.iter().any(|c| c.name == "clear"),
            "'cl' should fuzzy-match /clear"
        );
    }

    // ── test_slash_menu_arrow_navigation ─────────────────────────────────────

    #[test]
    fn test_slash_menu_arrow_navigation() {
        let mut menu = SlashMenu::new();
        assert_eq!(menu.selected, 0);

        // Down moves selection forward.
        menu.handle_key(down_key());
        assert_eq!(menu.selected, 1);

        menu.handle_key(down_key());
        assert_eq!(menu.selected, 2);

        // Up moves selection backward.
        menu.handle_key(up_key());
        assert_eq!(menu.selected, 1);

        // Up from 0 wraps to last.
        menu.selected = 0;
        menu.handle_key(up_key());
        let count = BUILTIN_COMMANDS.len();
        assert_eq!(menu.selected, count - 1);

        // Down from last wraps to 0.
        menu.handle_key(down_key());
        assert_eq!(menu.selected, 0);
    }

    #[test]
    fn test_slash_menu_enter_returns_select() {
        let mut menu = SlashMenu::new();
        menu.selected = 0; // first command is /help
        let action = menu.handle_key(enter_key());
        match action {
            OverlayAction::Select(name) => {
                assert!(name.starts_with('/'), "selected name should start with /");
            }
            other => panic!("Expected Select, got {:?}", other),
        }
    }

    #[test]
    fn test_slash_menu_query_typing() {
        let mut menu = SlashMenu::new();
        menu.handle_key(char_key('h'));
        assert_eq!(menu.query, "h");
        menu.handle_key(char_key('e'));
        assert_eq!(menu.query, "he");
        // Backspace removes last char.
        menu.handle_key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));
        assert_eq!(menu.query, "h");
    }
}
