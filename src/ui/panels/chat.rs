//! Chat panel — scrollable conversation history with markdown rendering and search.
//!
//! This module provides [`ChatPanel`], a self-contained panel that owns a copy
//! of the session transcript, manages scroll state, and provides incremental
//! text search. It implements the [`Panel`] trait from [`super`].

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    Frame,
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Widget},
};

use crate::app::state::{AppState, MessageRole, TranscriptEntry};
use crate::ui::markdown::{extract_lang, is_code_fence, render_markdown_line};
use crate::ui::theme::{AMBER, BG, BRASS, BROWN, CHARCOAL, CREAM, ROSE, STONE, TAN};

use super::{Panel, PanelAction, PanelId};

// ── ChatSearch ────────────────────────────────────────────────────────────────

/// Search state for the chat panel's in-panel search.
#[derive(Debug, Clone, Default)]
pub struct ChatSearch {
    /// Current search query.
    pub query: String,
    /// Whether the search bar is open and receiving input.
    pub active: bool,
    /// All matches as `(entry_idx, line_idx)` pairs.
    pub matches: Vec<(usize, usize)>,
    /// Index into `matches` for the "current" highlighted match.
    pub current: usize,
}

// ── ChatPanel ─────────────────────────────────────────────────────────────────

/// The primary chat panel — renders the session transcript with markdown,
/// search highlights, timestamps, and code-block shading.
#[derive(Debug, Default)]
pub struct ChatPanel {
    /// Local copy of the transcript (synced from session state).
    pub transcript: Vec<TranscriptEntry>,
    /// Lines-from-the-bottom scroll offset; 0 = pinned to bottom.
    pub scroll_offset: u16,
    /// Whether the user has manually scrolled up. Suppresses auto-scroll.
    pub user_scrolled: bool,
    /// Search state (None = never opened, Some = opened at least once).
    pub search: Option<ChatSearch>,
    /// Whether to show timestamps on each message.
    pub show_timestamps: bool,
    /// Whether this panel is visible.
    visible: bool,
}

impl ChatPanel {
    /// Create a new `ChatPanel` with the given initial transcript.
    ///
    /// The scroll is pinned to the bottom (`scroll_offset = 0`,
    /// `user_scrolled = false`).
    pub fn new(transcript: Vec<TranscriptEntry>) -> Self {
        Self {
            transcript,
            scroll_offset: 0,
            user_scrolled: false,
            search: None,
            show_timestamps: false,
            visible: true,
        }
    }

    // ── Transcript sync ───────────────────────────────────────────────────────

    /// Replace the stored transcript with a new one from session state.
    ///
    /// If the user has not scrolled up (`user_scrolled == false`) the view is
    /// automatically pinned to the bottom after the update.
    pub fn update_transcript(&mut self, transcript: Vec<TranscriptEntry>) {
        self.transcript = transcript;
        if !self.user_scrolled {
            self.scroll_to_bottom();
        }
        // Re-run search in case matches changed.
        self.find_matches_internal();
    }

    // ── Scroll helpers ────────────────────────────────────────────────────────

    /// Scroll up by `lines`. Marks `user_scrolled = true`.
    pub fn scroll_up(&mut self, lines: u16) {
        self.scroll_offset = self.scroll_offset.saturating_add(lines);
        self.user_scrolled = true;
    }

    /// Scroll down by `lines`. If the view reaches the bottom, resets
    /// `user_scrolled` to `false`.
    pub fn scroll_down(&mut self, lines: u16) {
        if self.scroll_offset <= lines {
            self.scroll_offset = 0;
            self.user_scrolled = false;
        } else {
            self.scroll_offset -= lines;
        }
    }

    /// Pin the view to the bottom and clear the user-scrolled flag.
    pub fn scroll_to_bottom(&mut self) {
        self.scroll_offset = 0;
        self.user_scrolled = false;
    }

    // ── Search helpers ────────────────────────────────────────────────────────

    /// Open the search bar (empty query, active = true).
    pub fn open_search(&mut self) {
        let search = self.search.get_or_insert_with(ChatSearch::default);
        search.query.clear();
        search.active = true;
        search.matches.clear();
        search.current = 0;
    }

    /// Close the search bar.
    pub fn close_search(&mut self) {
        if let Some(ref mut s) = self.search {
            s.active = false;
        }
    }

    /// Append a character to the search query and re-run the search.
    pub fn search_query_push(&mut self, c: char) {
        if let Some(ref mut s) = self.search {
            s.query.push(c);
        }
        self.find_matches_internal();
    }

    /// Remove the last character from the search query and re-run the search.
    pub fn search_query_pop(&mut self) {
        if let Some(ref mut s) = self.search {
            s.query.pop();
        }
        self.find_matches_internal();
    }

    /// Advance to the next search match (wraps around).
    pub fn search_next(&mut self) {
        if let Some(ref mut s) = self.search {
            if !s.matches.is_empty() {
                s.current = (s.current + 1) % s.matches.len();
            }
        }
    }

    /// Go to the previous search match (wraps around).
    pub fn search_prev(&mut self) {
        if let Some(ref mut s) = self.search {
            if !s.matches.is_empty() {
                s.current = if s.current == 0 {
                    s.matches.len() - 1
                } else {
                    s.current - 1
                };
            }
        }
    }

    /// Scan the transcript for the current query and populate `matches`.
    ///
    /// Matching is case-insensitive substring search. If the query is empty,
    /// matches are cleared.
    pub fn find_matches(&mut self) {
        self.find_matches_internal();
    }

    // ── Internal helpers ──────────────────────────────────────────────────────

    fn find_matches_internal(&mut self) {
        let query = match self.search.as_ref() {
            Some(s) if !s.query.is_empty() => s.query.to_lowercase(),
            _ => {
                if let Some(ref mut s) = self.search {
                    s.matches.clear();
                    s.current = 0;
                }
                return;
            }
        };

        let mut matches: Vec<(usize, usize)> = Vec::new();
        for (entry_idx, entry) in self.transcript.iter().enumerate() {
            let content_lower = entry.content.to_lowercase();
            for (line_idx, line) in content_lower.lines().enumerate() {
                if line.contains(&*query) {
                    matches.push((entry_idx, line_idx));
                }
            }
        }

        if let Some(ref mut s) = self.search {
            if matches.is_empty() {
                s.current = 0;
            } else if s.current >= matches.len() {
                s.current = matches.len() - 1;
            }
            s.matches = matches;
        }
    }

    // ── Rendering helpers ─────────────────────────────────────────────────────

    /// Build all visible [`Line`]s for the current transcript.
    fn build_lines(&self) -> Vec<Line<'static>> {
        let mut lines: Vec<Line<'static>> = Vec::new();
        let search_query = self.search.as_ref().and_then(|s| {
            if !s.query.is_empty() {
                Some(s.query.to_lowercase())
            } else {
                None
            }
        });
        let current_match = self.search.as_ref().map(|s| s.current);
        let all_matches = self.search.as_ref().map(|s| &s.matches);

        let mut in_code_block = false;

        for (entry_idx, entry) in self.transcript.iter().enumerate() {
            let prefix_span = role_prefix(entry);
            let ts_span = if self.show_timestamps {
                Some(Span::styled(
                    format!(" {}", entry.timestamp.format("%H:%M")),
                    Style::default().fg(STONE).add_modifier(Modifier::DIM),
                ))
            } else {
                None
            };

            let content = entry.content.clone();
            let content_lines: Vec<&str> = content.lines().collect();
            let total_content_lines = content_lines.len().max(1);

            for (line_idx, raw_line) in content_lines.iter().enumerate() {
                if is_code_fence(raw_line) {
                    in_code_block = !in_code_block;
                    let lang = extract_lang(raw_line);
                    let fence_label = if lang.is_empty() {
                        "```".to_string()
                    } else {
                        format!("```{}", lang)
                    };
                    lines.push(Line::from(Span::styled(
                        fence_label,
                        Style::default().fg(BRASS),
                    )));
                    continue;
                }

                let is_match = search_query.is_some()
                    && all_matches
                        .map(|m| m.contains(&(entry_idx, line_idx)))
                        .unwrap_or(false);

                let is_current_match = is_match && {
                    let mut found = false;
                    if let Some(matches) = all_matches {
                        for (i, &(ei, li)) in matches.iter().enumerate() {
                            if ei == entry_idx && li == line_idx {
                                if current_match == Some(i) {
                                    found = true;
                                }
                                break;
                            }
                        }
                    }
                    found
                };

                let line = if in_code_block {
                    let mut spans: Vec<Span<'static>> = Vec::new();
                    if line_idx == 0 {
                        spans.push(prefix_span.clone());
                    } else {
                        spans.push(Span::raw("  "));
                    }
                    spans.push(Span::styled(
                        format!("  {}", raw_line),
                        Style::default().fg(CREAM).bg(CHARCOAL),
                    ));
                    Line::from(spans)
                } else {
                    let md_spans = if line_idx == 0 {
                        let mut s = vec![prefix_span.clone()];
                        s.extend(render_markdown_line(raw_line));
                        s
                    } else {
                        let mut s = vec![Span::raw("  ")];
                        s.extend(render_markdown_line(raw_line));
                        s
                    };

                    if is_current_match {
                        let highlighted: Vec<Span<'static>> = md_spans
                            .into_iter()
                            .map(|s| Span::styled(s.content, s.style.bg(AMBER).fg(BG)))
                            .collect();
                        Line::from(highlighted)
                    } else if is_match {
                        let highlighted: Vec<Span<'static>> = md_spans
                            .into_iter()
                            .map(|s| Span::styled(s.content, s.style.bg(BROWN)))
                            .collect();
                        Line::from(highlighted)
                    } else {
                        Line::from(md_spans)
                    }
                };

                if line_idx == total_content_lines - 1 {
                    if let Some(ref ts) = ts_span {
                        let mut extended = line;
                        extended.spans.push(ts.clone());
                        lines.push(extended);
                        continue;
                    }
                }

                lines.push(line);
            }

            if content_lines.is_empty() {
                lines.push(Line::from(prefix_span.clone()));
            }

            lines.push(Line::from(""));
        }

        lines
    }

    /// Handle a key press while the search bar is active.
    fn handle_key_search(&mut self, key: KeyEvent) -> PanelAction {
        match key.code {
            KeyCode::Esc | KeyCode::Enter => {
                self.close_search();
            }
            KeyCode::Backspace => {
                self.search_query_pop();
            }
            KeyCode::Char('n') => {
                self.search_next();
            }
            KeyCode::Char('N') => {
                self.search_prev();
            }
            KeyCode::Char(c) => {
                self.search_query_push(c);
            }
            _ => {}
        }
        PanelAction::None
    }

    /// Core drawing function.
    pub(crate) fn draw(&self, buf: &mut Buffer, area: Rect) {
        for y in area.top()..area.bottom() {
            for x in area.left()..area.right() {
                buf[(x, y)].set_style(Style::default().bg(BG));
            }
        }

        if area.height == 0 || area.width == 0 {
            return;
        }

        let lines = self.build_lines();
        let total = lines.len() as u16;
        let height = area.height;

        let scroll_from_top: u16 = if total > height {
            let max_scroll = total - height;
            max_scroll.saturating_sub(self.scroll_offset)
        } else {
            0
        };

        for (i, line) in lines.iter().enumerate() {
            let virt_y = i as u16;
            if virt_y < scroll_from_top {
                continue;
            }
            let screen_y = area.y + virt_y - scroll_from_top;
            if screen_y >= area.bottom() {
                break;
            }
            render_line_at(buf, line, area.x, screen_y, area.width);
        }

        if self.user_scrolled && self.scroll_offset > 0 {
            let label = format!(" \u{2193} {} below ", self.scroll_offset);
            let lx = area.right().saturating_sub(label.len() as u16 + 1);
            let ly = area.bottom().saturating_sub(1);
            let style = Style::default().fg(AMBER).bg(CHARCOAL);
            for (i, ch) in label.chars().enumerate() {
                let cx = lx + i as u16;
                if cx >= area.right() {
                    break;
                }
                buf[(cx, ly)].set_char(ch).set_style(style);
            }
        }

        if let Some(ref s) = self.search {
            if s.active {
                let bar_y = area.bottom().saturating_sub(1);
                let query_display = format!(
                    " / {} ({}/{}) ",
                    s.query,
                    if s.matches.is_empty() { 0 } else { s.current + 1 },
                    s.matches.len()
                );
                let style = Style::default().fg(CREAM).bg(CHARCOAL);
                for (i, ch) in query_display.chars().enumerate() {
                    let cx = area.x + i as u16;
                    if cx >= area.right() {
                        break;
                    }
                    buf[(cx, bar_y)].set_char(ch).set_style(style);
                }
            }
        }
    }
}

// ── Role prefix helper ────────────────────────────────────────────────────────

fn role_prefix(entry: &TranscriptEntry) -> Span<'static> {
    match entry.role {
        MessageRole::User => Span::styled(
            "\u{276f} ".to_string(),
            Style::default().fg(AMBER).add_modifier(Modifier::BOLD),
        ),
        MessageRole::Assistant => Span::styled("  ".to_string(), Style::default()),
        MessageRole::System => Span::styled(
            "\u{2022} ".to_string(),
            Style::default().fg(BRASS),
        ),
        MessageRole::Error => Span::styled(
            "\u{2717} ".to_string(),
            Style::default().fg(ROSE),
        ),
    }
}

// ── Panel impl ────────────────────────────────────────────────────────────────

impl Panel for ChatPanel {
    fn id(&self) -> PanelId {
        PanelId::Chat
    }

    fn title(&self) -> &str {
        "Chat"
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
            .title(Span::styled(" Chat ", Style::default().fg(TAN)))
            .style(Style::default().bg(BG));

        let inner = block.inner(area);
        frame.render_widget(block, area);

        let buf = frame.buffer_mut();
        self.draw(buf, inner);
    }

    fn handle_key(&mut self, key: KeyEvent, _state: &mut AppState) -> PanelAction {
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            return PanelAction::None;
        }

        if let Some(ref s) = self.search {
            if s.active {
                return self.handle_key_search(key);
            }
        }

        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                self.scroll_up(3);
                PanelAction::None
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.scroll_down(3);
                PanelAction::None
            }
            KeyCode::PageUp => {
                self.scroll_up(10);
                PanelAction::None
            }
            KeyCode::PageDown => {
                self.scroll_down(10);
                PanelAction::None
            }
            KeyCode::Char('G') => {
                self.scroll_to_bottom();
                PanelAction::None
            }
            KeyCode::Char('T') => {
                self.show_timestamps = !self.show_timestamps;
                PanelAction::None
            }
            KeyCode::Char('/') => {
                self.open_search();
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

// ── Line renderer helper ──────────────────────────────────────────────────────

fn render_line_at(buf: &mut Buffer, line: &Line<'_>, x: u16, y: u16, max_width: u16) {
    let mut cx = x;
    for span in &line.spans {
        for ch in span.content.chars() {
            if cx >= x + max_width {
                return;
            }
            buf[(cx, y)].set_char(ch).set_style(span.style);
            cx += 1;
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::state::TranscriptEntry;

    fn user_entry(content: &str) -> TranscriptEntry {
        TranscriptEntry {
            role: MessageRole::User,
            content: content.to_string(),
            timestamp: chrono::Utc::now(),
            tool_call: None,
        }
    }

    fn assistant_entry(content: &str) -> TranscriptEntry {
        TranscriptEntry {
            role: MessageRole::Assistant,
            content: content.to_string(),
            timestamp: chrono::Utc::now(),
            tool_call: None,
        }
    }

    // ── Construction ─────────────────────────────────────────────────────────

    #[test]
    fn new_panel_has_empty_transcript() {
        let panel = ChatPanel::new(vec![]);
        assert!(panel.transcript.is_empty());
        assert_eq!(panel.scroll_offset, 0);
        assert!(!panel.user_scrolled);
    }

    #[test]
    fn new_panel_pinned_to_bottom() {
        let panel = ChatPanel::new(vec![user_entry("hi")]);
        assert_eq!(panel.scroll_offset, 0);
        assert!(!panel.user_scrolled);
    }

    // ── update_transcript ─────────────────────────────────────────────────────

    #[test]
    fn update_transcript_auto_scrolls_when_not_user_scrolled() {
        let mut panel = ChatPanel::new(vec![]);
        panel.scroll_offset = 0;
        panel.user_scrolled = false;

        panel.update_transcript(vec![user_entry("a"), assistant_entry("b")]);

        assert_eq!(panel.scroll_offset, 0);
        assert!(!panel.user_scrolled);
        assert_eq!(panel.transcript.len(), 2);
    }

    #[test]
    fn update_transcript_does_not_auto_scroll_when_user_scrolled() {
        let mut panel = ChatPanel::new(vec![user_entry("first message")]);
        panel.scroll_up(5);
        assert!(panel.user_scrolled);
        let offset_before = panel.scroll_offset;

        panel.update_transcript(vec![
            user_entry("first message"),
            assistant_entry("new message"),
        ]);

        assert_eq!(panel.scroll_offset, offset_before);
        assert!(panel.user_scrolled);
    }

    // ── Scroll ────────────────────────────────────────────────────────────────

    #[test]
    fn scroll_up_sets_user_scrolled() {
        let mut panel = ChatPanel::new(vec![]);
        assert!(!panel.user_scrolled);
        panel.scroll_up(3);
        assert!(panel.user_scrolled);
        assert_eq!(panel.scroll_offset, 3);
    }

    #[test]
    fn scroll_down_to_bottom_clears_user_scrolled() {
        let mut panel = ChatPanel::new(vec![]);
        panel.scroll_up(10);
        assert!(panel.user_scrolled);
        panel.scroll_down(10);
        assert_eq!(panel.scroll_offset, 0);
        assert!(!panel.user_scrolled);
    }

    #[test]
    fn scroll_down_partial_stays_user_scrolled() {
        let mut panel = ChatPanel::new(vec![]);
        panel.scroll_up(10);
        panel.scroll_down(3);
        assert_eq!(panel.scroll_offset, 7);
        assert!(panel.user_scrolled);
    }

    #[test]
    fn scroll_to_bottom_resets_offset_and_flag() {
        let mut panel = ChatPanel::new(vec![]);
        panel.scroll_up(20);
        panel.scroll_to_bottom();
        assert_eq!(panel.scroll_offset, 0);
        assert!(!panel.user_scrolled);
    }

    // ── Search: open / close ──────────────────────────────────────────────────

    #[test]
    fn open_search_sets_active() {
        let mut panel = ChatPanel::new(vec![]);
        panel.open_search();
        let s = panel.search.as_ref().unwrap();
        assert!(s.active);
        assert!(s.query.is_empty());
    }

    #[test]
    fn close_search_clears_active() {
        let mut panel = ChatPanel::new(vec![]);
        panel.open_search();
        panel.close_search();
        let s = panel.search.as_ref().unwrap();
        assert!(!s.active);
    }

    // ── Search: query push/pop ────────────────────────────────────────────────

    #[test]
    fn search_query_push_pop() {
        let mut panel = ChatPanel::new(vec![]);
        panel.open_search();
        panel.search_query_push('h');
        panel.search_query_push('i');
        {
            let s = panel.search.as_ref().unwrap();
            assert_eq!(s.query, "hi");
        }
        panel.search_query_pop();
        {
            let s = panel.search.as_ref().unwrap();
            assert_eq!(s.query, "h");
        }
        panel.search_query_pop();
        {
            let s = panel.search.as_ref().unwrap();
            assert!(s.query.is_empty());
        }
        panel.search_query_pop();
        {
            let s = panel.search.as_ref().unwrap();
            assert!(s.query.is_empty());
        }
    }

    // ── Search: find_matches ──────────────────────────────────────────────────

    #[test]
    fn find_matches_case_insensitive() {
        let mut panel = ChatPanel::new(vec![
            user_entry("Hello World"),
            assistant_entry("world peace"),
            user_entry("nothing here"),
        ]);
        panel.open_search();
        for c in "world".chars() { panel.search_query_push(c); }

        let s = panel.search.as_ref().unwrap();
        assert_eq!(s.matches.len(), 2);
        assert!(s.matches.contains(&(0, 0)));
        assert!(s.matches.contains(&(1, 0)));
    }

    #[test]
    fn find_matches_empty_query_clears_matches() {
        let mut panel = ChatPanel::new(vec![user_entry("hello world")]);
        panel.open_search();
        panel.search_query_push('h');
        {
            let s = panel.search.as_ref().unwrap();
            assert!(!s.matches.is_empty());
        }
        panel.search_query_pop();
        {
            let s = panel.search.as_ref().unwrap();
            assert!(s.matches.is_empty());
        }
    }

    #[test]
    fn find_matches_multiline_content() {
        let mut panel = ChatPanel::new(vec![
            assistant_entry("line one\nline two\nline THREE"),
        ]);
        panel.open_search();
        for c in "three".chars() { panel.search_query_push(c); }

        let s = panel.search.as_ref().unwrap();
        assert!(s.matches.contains(&(0, 2)));
    }

    // ── Search: next / prev wrapping ──────────────────────────────────────────

    #[test]
    fn search_next_wraps() {
        let mut panel = ChatPanel::new(vec![
            user_entry("foo"),
            assistant_entry("foo bar"),
            user_entry("foo baz"),
        ]);
        panel.open_search();
        for c in "foo".chars() { panel.search_query_push(c); }
        {
            let s = panel.search.as_ref().unwrap();
            assert_eq!(s.matches.len(), 3);
            assert_eq!(s.current, 0);
        }

        panel.search_next();
        assert_eq!(panel.search.as_ref().unwrap().current, 1);
        panel.search_next();
        assert_eq!(panel.search.as_ref().unwrap().current, 2);
        panel.search_next();
        assert_eq!(panel.search.as_ref().unwrap().current, 0);
    }

    #[test]
    fn search_prev_wraps() {
        let mut panel = ChatPanel::new(vec![
            user_entry("foo"),
            assistant_entry("foo bar"),
        ]);
        panel.open_search();
        for c in "foo".chars() { panel.search_query_push(c); }
        {
            let s = panel.search.as_ref().unwrap();
            assert_eq!(s.matches.len(), 2);
            assert_eq!(s.current, 0);
        }

        panel.search_prev();
        assert_eq!(panel.search.as_ref().unwrap().current, 1);
        panel.search_prev();
        assert_eq!(panel.search.as_ref().unwrap().current, 0);
    }

    #[test]
    fn search_next_no_op_when_no_matches() {
        let mut panel = ChatPanel::new(vec![user_entry("hello")]);
        panel.open_search();
        panel.search_query_push('z');
        let before = panel.search.as_ref().unwrap().current;
        panel.search_next();
        assert_eq!(panel.search.as_ref().unwrap().current, before);
    }

    #[test]
    fn search_prev_no_op_when_no_matches() {
        let mut panel = ChatPanel::new(vec![user_entry("hello")]);
        panel.open_search();
        panel.search_query_push('z');
        let before = panel.search.as_ref().unwrap().current;
        panel.search_prev();
        assert_eq!(panel.search.as_ref().unwrap().current, before);
    }

    // ── Visibility ────────────────────────────────────────────────────────────

    #[test]
    fn visibility_toggle() {
        let mut panel = ChatPanel::new(vec![]);
        assert!(panel.is_visible());
        panel.set_visible(false);
        assert!(!panel.is_visible());
        panel.set_visible(true);
        assert!(panel.is_visible());
    }

    // ── Panel trait basics ────────────────────────────────────────────────────

    #[test]
    fn panel_id_is_chat() {
        let panel = ChatPanel::new(vec![]);
        assert_eq!(panel.id(), PanelId::Chat);
    }

    #[test]
    fn panel_title_is_chat() {
        let panel = ChatPanel::new(vec![]);
        assert_eq!(panel.title(), "Chat");
    }
}
