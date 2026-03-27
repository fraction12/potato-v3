//! Tool output panel — live streaming output from tool executions.
//!
//! Each tool call is a collapsible section with a header showing:
//!   - Status icon: ✓ done, ✗ error, ⏳ running
//!   - Tool name
//!   - Duration
//!   - Collapsible body (input JSON + output text)
//!
//! Keys (when focused):
//!   - `C`       — clear all history
//!   - Enter     — toggle collapse on the selected tool section
//!   - ↑/k       — navigate up
//!   - ↓/j       — navigate down
//!   - PageUp    — scroll up 10 lines
//!   - PageDown  — scroll down 10 lines

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Widget},
};
use chrono::{DateTime, Utc};

use crate::app::state::{AppState, ToolCallRecord};
use crate::ui::theme::{AMBER, BG, CHARCOAL, CREAM, RUST_RED, SOIL, SPROUT, TAN};

use super::{Panel, PanelAction, PanelId};

// ── ToolOutputEntry ───────────────────────────────────────────────────────────

/// A single tool-execution entry shown in the panel.
///
/// Created from a [`ToolCallRecord`] and updated in-place as the tool runs.
#[derive(Debug, Clone)]
pub struct ToolOutputEntry {
    /// Unique identifier (matches [`ToolCallRecord::id`]).
    pub id: String,
    /// Human-readable tool name.
    pub name: String,
    /// Input parameters as JSON.
    pub input: serde_json::Value,
    /// Accumulated output text (None while running).
    pub output: Option<String>,
    /// Execution duration in milliseconds (None while running).
    pub duration_ms: Option<u64>,
    /// Whether the tool invocation succeeded (None while running).
    pub success: Option<bool>,
    /// Whether this entry's body is collapsed in the UI.
    pub collapsed: bool,
    /// When the tool was started.
    pub started_at: DateTime<Utc>,
}

impl ToolOutputEntry {
    /// Build a new entry from a [`ToolCallRecord`]; defaults collapsed to `false`.
    pub fn from_record(record: &ToolCallRecord) -> Self {
        Self {
            id: record.id.clone(),
            name: record.name.clone(),
            input: record.input.clone(),
            output: record.output.clone(),
            duration_ms: record.duration_ms,
            success: record.success,
            collapsed: false,
            started_at: record.started_at,
        }
    }

    /// Format a duration as `"42ms"` or `"1.3s"`.
    pub fn duration_str(&self) -> String {
        match self.duration_ms {
            None => "…".to_string(),
            Some(ms) if ms < 1000 => format!("{}ms", ms),
            Some(ms) => format!("{:.1}s", ms as f64 / 1000.0),
        }
    }
}

// ── ToolOutputPanel ───────────────────────────────────────────────────────────

/// Panel that shows a live, collapsible timeline of every tool execution.
#[derive(Debug, Default)]
pub struct ToolOutputPanel {
    /// Ordered list of tool execution entries.
    entries: Vec<ToolOutputEntry>,
    /// Vertical scroll offset (lines from the top of the rendered list).
    scroll_offset: u16,
    /// Index of the currently selected entry.
    selected: usize,
    /// Whether this panel is visible.
    visible: bool,
}

impl ToolOutputPanel {
    /// Create an empty, visible panel.
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            scroll_offset: 0,
            selected: 0,
            visible: true,
        }
    }

    // ── Accessors (pub for tests) ─────────────────────────────────────────────

    /// How many entries are currently recorded.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns `true` if there are no entries.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Immutable slice of all entries.
    pub fn entries(&self) -> &[ToolOutputEntry] {
        &self.entries
    }

    /// Index of the currently selected entry.
    pub fn selected(&self) -> usize {
        self.selected
    }

    /// Current scroll offset.
    pub fn scroll_offset(&self) -> u16 {
        self.scroll_offset
    }

    // ── Mutation ──────────────────────────────────────────────────────────────

    /// Append a new entry from a [`ToolCallRecord`].
    ///
    /// The new entry is always added with `collapsed = false`.
    pub fn add_entry(&mut self, record: &ToolCallRecord) {
        self.entries.push(ToolOutputEntry::from_record(record));
        // Auto-select the newest entry.
        self.selected = self.entries.len().saturating_sub(1);
    }

    /// Update an existing entry by `id`.
    ///
    /// If no entry with the given `id` exists this is a no-op.
    pub fn update_entry(
        &mut self,
        id: &str,
        output: Option<String>,
        duration_ms: Option<u64>,
        success: Option<bool>,
    ) {
        if let Some(entry) = self.entries.iter_mut().find(|e| e.id == id) {
            entry.output = output;
            entry.duration_ms = duration_ms;
            entry.success = success;
        }
    }

    /// Toggle the collapsed state of the currently selected entry.
    pub fn toggle_collapse(&mut self) {
        if let Some(entry) = self.entries.get_mut(self.selected) {
            entry.collapsed = !entry.collapsed;
        }
    }

    /// Move selection to the next entry, clamping at the last.
    pub fn select_next(&mut self) {
        if self.selected + 1 < self.entries.len() {
            self.selected += 1;
        }
    }

    /// Move selection to the previous entry, clamping at zero.
    pub fn select_prev(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }

    /// Remove all entries and reset selection/scroll.
    pub fn clear(&mut self) {
        self.entries.clear();
        self.selected = 0;
        self.scroll_offset = 0;
    }

    /// Scroll up by `lines`.
    pub fn scroll_up(&mut self, lines: u16) {
        self.scroll_offset = self.scroll_offset.saturating_sub(lines);
    }

    /// Scroll down by `lines` (no upper bound — render clips naturally).
    pub fn scroll_down(&mut self, lines: u16) {
        self.scroll_offset = self.scroll_offset.saturating_add(lines);
    }

    // ── Legacy API (kept for backward-compatibility with existing callers) ────

    /// Append a new *running* record by name alone (legacy push API).
    pub fn push_record(&mut self, name: impl Into<String>) {
        let name = name.into();
        let record = ToolCallRecord {
            id: uuid_from_name(&name),
            name,
            input: serde_json::Value::Null,
            output: None,
            started_at: Utc::now(),
            duration_ms: None,
            success: None,
        };
        self.add_entry(&record);
    }

    /// Append output to the most recent running record with the given name (legacy).
    pub fn append_output(&mut self, name: &str, chunk: &str) {
        if let Some(entry) = self
            .entries
            .iter_mut()
            .rev()
            .find(|e| e.name == name && e.success.is_none())
        {
            let buf = entry.output.get_or_insert_with(String::new);
            buf.push_str(chunk);
        }
    }

    /// Mark the most recent running record with the given name as done/failed (legacy).
    pub fn finish_record(&mut self, name: &str, success: bool) {
        if let Some(entry) = self
            .entries
            .iter_mut()
            .rev()
            .find(|e| e.name == name && e.success.is_none())
        {
            entry.success = Some(success);
            entry.duration_ms = Some(
                Utc::now()
                    .signed_duration_since(entry.started_at)
                    .num_milliseconds()
                    .max(0) as u64,
            );
        }
    }

    /// Toggle collapse for the currently selected entry (legacy alias).
    pub fn toggle_selected(&mut self) {
        self.toggle_collapse();
    }
}

// ── Panel trait ───────────────────────────────────────────────────────────────

impl Panel for ToolOutputPanel {
    fn id(&self) -> PanelId {
        PanelId::ToolOutput
    }

    fn title(&self) -> &str {
        "Tools"
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
            .title(Span::styled(" Tools ", Style::default().fg(TAN)))
            .style(Style::default().bg(BG));

        let inner = block.inner(area);
        frame.render_widget(block, area);

        if self.entries.is_empty() {
            let hint = Paragraph::new(Span::styled(
                " No tool calls yet.",
                Style::default().fg(SOIL),
            ))
            .style(Style::default().bg(BG));
            hint.render(inner, frame.buffer_mut());
            return;
        }

        // Build all rendered lines for the full virtual list.
        let mut lines: Vec<Line<'static>> = Vec::new();
        for (idx, entry) in self.entries.iter().enumerate() {
            let is_selected = focused && idx == self.selected;
            lines.extend(entry_lines(entry, is_selected));
        }

        // Apply scroll offset.
        let skip = self.scroll_offset as usize;
        let visible: Vec<Line<'static>> = lines
            .into_iter()
            .skip(skip)
            .take(inner.height as usize)
            .collect();

        let para = Paragraph::new(visible).style(Style::default().bg(BG));
        para.render(inner, frame.buffer_mut());
    }

    fn handle_key(&mut self, key: KeyEvent, _state: &mut AppState) -> PanelAction {
        // Never intercept Ctrl-modified keys (those belong to the global handler).
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            return PanelAction::None;
        }
        match key.code {
            // Navigate up.
            KeyCode::Up | KeyCode::Char('k') => {
                self.select_prev();
                PanelAction::None
            }
            // Navigate down.
            KeyCode::Down | KeyCode::Char('j') => {
                self.select_next();
                PanelAction::None
            }
            // Toggle collapse on selected entry.
            KeyCode::Enter => {
                self.toggle_collapse();
                PanelAction::None
            }
            // Uppercase C — clear history.
            KeyCode::Char('C') => {
                self.clear();
                PanelAction::None
            }
            // Page scroll.
            KeyCode::PageUp => {
                self.scroll_up(10);
                PanelAction::None
            }
            KeyCode::PageDown => {
                self.scroll_down(10);
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

// ── Rendering helpers ─────────────────────────────────────────────────────────

/// Produce styled ratatui `Line`s for one entry.
fn entry_lines(entry: &ToolOutputEntry, selected: bool) -> Vec<Line<'static>> {
    let (icon, icon_style) = status_icon_and_style(entry);
    let collapse_hint = if entry.collapsed { "▸ " } else { "▾ " };
    let sel_marker = if selected { "❯ " } else { "  " };
    let sel_style = if selected {
        Style::default().fg(AMBER).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(SOIL)
    };

    // Header: `[sel] [collapse] [icon] name  (duration)`
    let header = Line::from(vec![
        Span::styled(sel_marker.to_string(), sel_style),
        Span::styled(collapse_hint.to_string(), Style::default().fg(SOIL)),
        Span::styled(format!("{} ", icon), icon_style),
        Span::styled(entry.name.clone(), icon_style),
        Span::raw("  "),
        Span::styled(entry.duration_str(), Style::default().fg(SOIL)),
    ]);

    if entry.collapsed {
        return vec![header];
    }

    let mut lines = vec![header];

    // Input JSON (max 5 lines).
    let input_str = serde_json::to_string_pretty(&entry.input)
        .unwrap_or_else(|_| entry.input.to_string());
    let input_lines: Vec<&str> = input_str.lines().collect();
    let truncated_input = input_lines.len() > 5;
    for line in input_lines.iter().take(5) {
        lines.push(Line::from(Span::styled(
            format!("    {}", line),
            Style::default().fg(TAN),
        )));
    }
    if truncated_input {
        lines.push(Line::from(Span::styled(
            "    …".to_string(),
            Style::default().fg(SOIL),
        )));
    }

    // Output (max 10 lines).
    if let Some(ref output) = entry.output {
        let out_lines: Vec<&str> = output.lines().collect();
        let truncated_output = out_lines.len() > 10;
        for line in out_lines.iter().take(10) {
            lines.push(Line::from(Span::styled(
                format!("    {}", line),
                Style::default().fg(CREAM),
            )));
        }
        if truncated_output {
            lines.push(Line::from(Span::styled(
                "    …".to_string(),
                Style::default().fg(SOIL),
            )));
        }
    }

    lines
}

/// Returns `(icon_char, style)` for an entry based on its `success` field.
fn status_icon_and_style(entry: &ToolOutputEntry) -> (&'static str, Style) {
    match entry.success {
        Some(true) => ("✓", Style::default().fg(SPROUT)),
        Some(false) => ("✗", Style::default().fg(RUST_RED)),
        None => ("⏳", Style::default().fg(AMBER)),
    }
}

// ── Utility ───────────────────────────────────────────────────────────────────

/// Derive a deterministic placeholder id from a tool name (legacy push_record path).
fn uuid_from_name(name: &str) -> String {
    format!("legacy-{}-{}", name, Utc::now().timestamp_nanos_opt().unwrap_or(0))
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::state::ToolCallRecord;
    use chrono::Utc;
    use serde_json::json;

    // ── Fixture helpers ───────────────────────────────────────────────────────

    fn make_record(id: &str, name: &str) -> ToolCallRecord {
        ToolCallRecord {
            id: id.to_string(),
            name: name.to_string(),
            input: json!({ "path": "/tmp/test.txt" }),
            output: None,
            started_at: Utc::now(),
            duration_ms: None,
            success: None,
        }
    }

    // ── RED → GREEN tests ─────────────────────────────────────────────────────

    #[test]
    fn new_panel_is_empty() {
        let panel = ToolOutputPanel::new();
        assert!(panel.is_empty());
        assert_eq!(panel.len(), 0);
        assert_eq!(panel.selected(), 0);
        assert_eq!(panel.scroll_offset(), 0);
        assert!(panel.is_visible());
    }

    #[test]
    fn add_entry_increases_count() {
        let mut panel = ToolOutputPanel::new();
        assert_eq!(panel.len(), 0);

        panel.add_entry(&make_record("t1", "read_file"));
        assert_eq!(panel.len(), 1);

        panel.add_entry(&make_record("t2", "shell"));
        assert_eq!(panel.len(), 2);
    }

    #[test]
    fn update_entry_sets_output_and_success() {
        let mut panel = ToolOutputPanel::new();
        panel.add_entry(&make_record("t1", "read_file"));

        panel.update_entry("t1", Some("file contents".to_string()), Some(42), Some(true));

        let entry = &panel.entries()[0];
        assert_eq!(entry.output.as_deref(), Some("file contents"));
        assert_eq!(entry.duration_ms, Some(42));
        assert_eq!(entry.success, Some(true));
    }

    #[test]
    fn update_unknown_entry_is_noop() {
        let mut panel = ToolOutputPanel::new();
        panel.add_entry(&make_record("t1", "shell"));

        // Update a non-existent id — must not panic, must not change existing entry.
        panel.update_entry("does-not-exist", Some("output".to_string()), Some(1), Some(true));

        assert_eq!(panel.len(), 1);
        assert!(panel.entries()[0].output.is_none());
    }

    #[test]
    fn toggle_collapse_on_selected() {
        let mut panel = ToolOutputPanel::new();
        panel.add_entry(&make_record("t1", "write_file"));

        assert!(!panel.entries()[0].collapsed, "starts expanded");

        panel.toggle_collapse();
        assert!(panel.entries()[0].collapsed, "should be collapsed after toggle");

        panel.toggle_collapse();
        assert!(!panel.entries()[0].collapsed, "should be expanded after second toggle");
    }

    #[test]
    fn select_next_clamps_at_end() {
        let mut panel = ToolOutputPanel::new();
        panel.add_entry(&make_record("t1", "a"));
        panel.add_entry(&make_record("t2", "b"));
        panel.add_entry(&make_record("t3", "c"));
        // After 3 adds, selected should be at index 2 (last).
        assert_eq!(panel.selected(), 2);

        // select_next at the end is a no-op.
        panel.select_next();
        assert_eq!(panel.selected(), 2, "should clamp at end");
    }

    #[test]
    fn select_prev_clamps_at_zero() {
        let mut panel = ToolOutputPanel::new();
        panel.add_entry(&make_record("t1", "a"));
        panel.add_entry(&make_record("t2", "b"));

        // Navigate to start.
        panel.select_prev();
        assert_eq!(panel.selected(), 0);

        // select_prev at zero is a no-op.
        panel.select_prev();
        assert_eq!(panel.selected(), 0, "should clamp at zero");
    }

    #[test]
    fn clear_empties_entries() {
        let mut panel = ToolOutputPanel::new();
        panel.add_entry(&make_record("t1", "a"));
        panel.add_entry(&make_record("t2", "b"));
        assert_eq!(panel.len(), 2);

        panel.clear();
        assert!(panel.is_empty());
        assert_eq!(panel.selected(), 0);
        assert_eq!(panel.scroll_offset(), 0);
    }

    #[test]
    fn add_entry_from_tool_call_record() {
        let record = ToolCallRecord {
            id: "abc-123".to_string(),
            name: "search".to_string(),
            input: json!({ "query": "hello" }),
            output: Some("result".to_string()),
            started_at: Utc::now(),
            duration_ms: Some(100),
            success: Some(true),
        };

        let mut panel = ToolOutputPanel::new();
        panel.add_entry(&record);

        assert_eq!(panel.len(), 1);
        let entry = &panel.entries()[0];
        assert_eq!(entry.id, "abc-123");
        assert_eq!(entry.name, "search");
        assert_eq!(entry.output.as_deref(), Some("result"));
        assert_eq!(entry.duration_ms, Some(100));
        assert_eq!(entry.success, Some(true));
        assert!(!entry.collapsed);
    }

    #[test]
    fn scroll_up_and_down() {
        let mut panel = ToolOutputPanel::new();
        assert_eq!(panel.scroll_offset(), 0);

        panel.scroll_down(5);
        assert_eq!(panel.scroll_offset(), 5);

        panel.scroll_down(10);
        assert_eq!(panel.scroll_offset(), 15);

        panel.scroll_up(7);
        assert_eq!(panel.scroll_offset(), 8);

        // scroll_up saturates at 0.
        panel.scroll_up(100);
        assert_eq!(panel.scroll_offset(), 0);
    }

    // ── Behaviour / integration tests (from legacy) ───────────────────────────

    #[test]
    fn panel_id_is_tool_output() {
        let panel = ToolOutputPanel::new();
        assert_eq!(panel.id(), PanelId::ToolOutput);
    }

    #[test]
    fn title_is_tools() {
        let panel = ToolOutputPanel::new();
        assert_eq!(panel.title(), "Tools");
    }

    #[test]
    fn toggle_collapse_expands_and_collapses() {
        let mut panel = ToolOutputPanel::new();
        panel.push_record("grep");
        assert!(!panel.entries()[0].collapsed);
        panel.toggle_collapse();
        assert!(panel.entries()[0].collapsed);
        panel.toggle_collapse();
        assert!(!panel.entries()[0].collapsed);
    }

    #[test]
    fn navigation_with_j_k_keys() {
        let mut panel = ToolOutputPanel::new();
        panel.add_entry(&make_record("t1", "a"));
        panel.add_entry(&make_record("t2", "b"));
        panel.add_entry(&make_record("t3", "c"));

        // Start at index 2 (last added).
        panel.selected = 1;

        let mut state = AppState::default();
        let key_down = KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE);
        panel.handle_key(key_down, &mut state);
        assert_eq!(panel.selected(), 2);

        let key_up = KeyEvent::new(KeyCode::Char('k'), KeyModifiers::NONE);
        panel.handle_key(key_up, &mut state);
        assert_eq!(panel.selected(), 1);
    }

    #[test]
    fn c_key_clears_entries() {
        let mut panel = ToolOutputPanel::new();
        panel.add_entry(&make_record("t1", "shell"));
        panel.add_entry(&make_record("t2", "shell"));
        assert_eq!(panel.len(), 2);

        let mut state = AppState::default();
        let key = KeyEvent::new(KeyCode::Char('C'), KeyModifiers::NONE);
        panel.handle_key(key, &mut state);
        assert!(panel.is_empty());
    }

    #[test]
    fn enter_key_toggles_collapse() {
        let mut panel = ToolOutputPanel::new();
        panel.add_entry(&make_record("t1", "read_file"));
        assert!(!panel.entries()[0].collapsed);

        let mut state = AppState::default();
        let key = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
        panel.handle_key(key, &mut state);
        assert!(panel.entries()[0].collapsed);
    }

    #[test]
    fn page_up_and_down_via_keys() {
        let mut panel = ToolOutputPanel::new();
        let mut state = AppState::default();

        panel.handle_key(KeyEvent::new(KeyCode::PageDown, KeyModifiers::NONE), &mut state);
        assert_eq!(panel.scroll_offset(), 10);

        panel.handle_key(KeyEvent::new(KeyCode::PageDown, KeyModifiers::NONE), &mut state);
        assert_eq!(panel.scroll_offset(), 20);

        panel.handle_key(KeyEvent::new(KeyCode::PageUp, KeyModifiers::NONE), &mut state);
        assert_eq!(panel.scroll_offset(), 10);
    }

    #[test]
    fn finish_record_marks_success() {
        let mut panel = ToolOutputPanel::new();
        panel.push_record("grep");
        panel.finish_record("grep", true);
        assert_eq!(panel.entries()[0].success, Some(true));
    }

    #[test]
    fn finish_record_marks_error() {
        let mut panel = ToolOutputPanel::new();
        panel.push_record("shell");
        panel.finish_record("shell", false);
        assert_eq!(panel.entries()[0].success, Some(false));
    }

    #[test]
    fn visibility_toggle() {
        let mut panel = ToolOutputPanel::new();
        assert!(panel.is_visible());
        panel.set_visible(false);
        assert!(!panel.is_visible());
        panel.set_visible(true);
        assert!(panel.is_visible());
    }

    #[test]
    fn duration_str_for_pending() {
        let entry = ToolOutputEntry {
            id: "x".into(),
            name: "test".into(),
            input: json!({}),
            output: None,
            duration_ms: None,
            success: None,
            collapsed: false,
            started_at: Utc::now(),
        };
        assert_eq!(entry.duration_str(), "…");
    }

    #[test]
    fn duration_str_for_ms() {
        let mut entry = ToolOutputEntry {
            id: "x".into(),
            name: "test".into(),
            input: json!({}),
            output: None,
            duration_ms: Some(42),
            success: Some(true),
            collapsed: false,
            started_at: Utc::now(),
        };
        assert_eq!(entry.duration_str(), "42ms");
        entry.duration_ms = Some(1500);
        assert_eq!(entry.duration_str(), "1.5s");
    }
}
