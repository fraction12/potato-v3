//! Tool output panel — live streaming output from tool executions.
//!
//! Each tool call is a collapsible section with a header showing:
//!   - Tool name
//!   - Status icon: ● running, ✓ done, ✗ error
//!   - Duration
//!   - Collapsible body (output text)
//!
//! Keys (when focused):
//!   - `C`     — clear all history
//!   - Enter   — toggle collapse on the selected tool section
//!   - ↑/↓     — navigate between tool sections
//!   - j/k     — same as ↑/↓

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Widget},
};
use chrono::{DateTime, Utc};

use crate::app::state::AppState;
use crate::ui::theme::{AMBER, BG, CHARCOAL, CREAM, RUST_RED, SOIL, SPROUT, TAN};

use super::{Panel, PanelAction, PanelId};

// ── Data model ────────────────────────────────────────────────────────────────

/// Status of a recorded tool execution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolStatus {
    Running,
    Done,
    Error,
}

/// A single tool-execution record shown in the panel.
#[derive(Debug, Clone)]
pub struct ToolRecord {
    /// Human-readable tool name.
    pub name: String,
    /// Current execution status.
    pub status: ToolStatus,
    /// Accumulated output text.
    pub output: String,
    /// When execution started.
    pub started_at: DateTime<Utc>,
    /// When execution finished (if applicable).
    pub finished_at: Option<DateTime<Utc>>,
    /// Whether the body is collapsed.
    pub collapsed: bool,
}

impl ToolRecord {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            status: ToolStatus::Running,
            output: String::new(),
            started_at: Utc::now(),
            finished_at: None,
            collapsed: false,
        }
    }

    /// Duration in milliseconds (uses now for running tools).
    pub fn duration_ms(&self) -> i64 {
        let end = self.finished_at.unwrap_or_else(Utc::now);
        end.signed_duration_since(self.started_at).num_milliseconds()
    }

    /// Duration string: "42ms" or "1.3s".
    pub fn duration_str(&self) -> String {
        let ms = self.duration_ms();
        if ms < 1000 {
            format!("{}ms", ms)
        } else {
            format!("{:.1}s", ms as f64 / 1000.0)
        }
    }
}

// ── ToolOutputPanel ───────────────────────────────────────────────────────────

/// Panel that shows live tool execution output.
#[derive(Debug, Default)]
pub struct ToolOutputPanel {
    /// History of tool executions.
    pub records: Vec<ToolRecord>,
    /// Currently selected record index (for navigation / collapse).
    pub selected: usize,
    /// Vertical scroll offset.
    pub scroll: usize,
    /// Whether this panel is visible.
    visible: bool,
}

impl ToolOutputPanel {
    pub fn new() -> Self {
        Self {
            records: Vec::new(),
            selected: 0,
            scroll: 0,
            visible: true,
        }
    }

    /// Append a new running tool record.
    pub fn push_record(&mut self, name: impl Into<String>) {
        self.records.push(ToolRecord::new(name));
        self.selected = self.records.len().saturating_sub(1);
    }

    /// Append output to the most recent record with the given name.
    pub fn append_output(&mut self, name: &str, chunk: &str) {
        if let Some(rec) = self
            .records
            .iter_mut()
            .rev()
            .find(|r| r.name == name && r.status == ToolStatus::Running)
        {
            rec.output.push_str(chunk);
        }
    }

    /// Mark a record as done.
    pub fn finish_record(&mut self, name: &str, success: bool) {
        if let Some(rec) = self
            .records
            .iter_mut()
            .rev()
            .find(|r| r.name == name && r.status == ToolStatus::Running)
        {
            rec.status = if success {
                ToolStatus::Done
            } else {
                ToolStatus::Error
            };
            rec.finished_at = Some(Utc::now());
        }
    }

    /// Clear all history.
    pub fn clear(&mut self) {
        self.records.clear();
        self.selected = 0;
        self.scroll = 0;
    }

    /// Toggle collapse for the currently selected record.
    pub fn toggle_selected(&mut self) {
        if let Some(rec) = self.records.get_mut(self.selected) {
            rec.collapsed = !rec.collapsed;
        }
    }
}

impl Panel for ToolOutputPanel {
    fn id(&self) -> PanelId {
        PanelId::ToolOutput
    }

    fn title(&self) -> &str {
        "Tool Output"
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
            .title(Span::styled(" Tool Output ", Style::default().fg(TAN)))
            .style(Style::default().bg(BG));

        let inner = block.inner(area);
        frame.render_widget(block, area);

        if self.records.is_empty() {
            let hint = Paragraph::new(Span::styled(
                " No tool executions yet.",
                Style::default().fg(SOIL),
            ))
            .style(Style::default().bg(BG));
            hint.render(inner, frame.buffer_mut());
            return;
        }

        // Build lines for all records.
        let mut lines: Vec<Line<'static>> = Vec::new();
        for (idx, rec) in self.records.iter().enumerate() {
            let is_selected = idx == self.selected && focused;
            lines.extend(record_lines(rec, is_selected));
        }

        // Simple scroll: show from `scroll` offset.
        let visible_lines: Vec<Line<'static>> = lines
            .into_iter()
            .skip(self.scroll)
            .take(inner.height as usize)
            .collect();

        let para = Paragraph::new(visible_lines).style(Style::default().bg(BG));
        para.render(inner, frame.buffer_mut());
    }

    fn handle_key(&mut self, key: KeyEvent, _state: &mut AppState) -> PanelAction {
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            return PanelAction::None;
        }
        match key.code {
            // Clear history
            KeyCode::Char('C') | KeyCode::Char('c')
                if key.modifiers.contains(KeyModifiers::SHIFT)
                    || key.code == KeyCode::Char('C') =>
            {
                self.clear();
                PanelAction::None
            }
            KeyCode::Char('c') => {
                // Lowercase c — only clear if shift is held (handled above).
                // Without shift, do nothing.
                PanelAction::None
            }
            // Toggle collapse
            KeyCode::Enter => {
                self.toggle_selected();
                PanelAction::None
            }
            // Navigate up
            KeyCode::Up | KeyCode::Char('k') => {
                if self.selected > 0 {
                    self.selected -= 1;
                }
                PanelAction::None
            }
            // Navigate down
            KeyCode::Down | KeyCode::Char('j') => {
                if self.selected + 1 < self.records.len() {
                    self.selected += 1;
                }
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

fn status_icon(status: &ToolStatus) -> &'static str {
    match status {
        ToolStatus::Running => "●",
        ToolStatus::Done => "✓",
        ToolStatus::Error => "✗",
    }
}

fn status_style(status: &ToolStatus) -> Style {
    match status {
        ToolStatus::Running => Style::default().fg(AMBER),
        ToolStatus::Done => Style::default().fg(SPROUT),
        ToolStatus::Error => Style::default().fg(RUST_RED),
    }
}

fn record_lines(rec: &ToolRecord, selected: bool) -> Vec<Line<'static>> {
    let st = status_style(&rec.status);
    let icon = status_icon(&rec.status);
    let collapse_hint = if rec.collapsed { "▸ " } else { "▾ " };
    let sel_marker = if selected { "❯ " } else { "  " };

    let header = Line::from(vec![
        Span::styled(sel_marker.to_string(), Style::default().fg(AMBER)),
        Span::styled(collapse_hint.to_string(), Style::default().fg(SOIL)),
        Span::styled(format!("{} ", icon), st),
        Span::styled(rec.name.clone(), st),
        Span::raw("  "),
        Span::styled(rec.duration_str(), Style::default().fg(SOIL)),
    ]);

    if rec.collapsed || rec.output.is_empty() {
        return vec![header];
    }

    let mut lines = vec![header];
    for out_line in rec.output.lines() {
        lines.push(Line::from(Span::styled(
            format!("    {}", out_line),
            Style::default().fg(CREAM),
        )));
    }
    lines
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Tool sections start uncollapsed; Enter toggles them.
    #[test]
    fn test_tool_output_panel_collapsible() {
        let mut panel = ToolOutputPanel::new();
        panel.push_record("read_file");
        panel.append_output("read_file", "line1\nline2");

        // Initially not collapsed.
        assert!(!panel.records[0].collapsed);

        // Toggle: should collapse.
        panel.toggle_selected();
        assert!(panel.records[0].collapsed);

        // Toggle again: should expand.
        panel.toggle_selected();
        assert!(!panel.records[0].collapsed);
    }

    /// Navigation with j/k / ↑↓ moves the selection.
    #[test]
    fn test_tool_output_navigation() {
        let mut panel = ToolOutputPanel::new();
        panel.push_record("tool_a");
        panel.push_record("tool_b");
        panel.push_record("tool_c");
        // selected should be at last after pushes
        assert_eq!(panel.selected, 2);

        // Up
        panel.selected = 1;
        // Navigate down
        let key_down = KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE);
        let mut state = AppState::default();
        panel.handle_key(key_down, &mut state);
        assert_eq!(panel.selected, 2);

        // Navigate up
        let key_up = KeyEvent::new(KeyCode::Char('k'), KeyModifiers::NONE);
        panel.handle_key(key_up, &mut state);
        assert_eq!(panel.selected, 1);
    }

    /// `C` (uppercase) clears all records.
    #[test]
    fn test_tool_output_clear() {
        let mut panel = ToolOutputPanel::new();
        panel.push_record("tool_a");
        panel.push_record("tool_b");
        assert_eq!(panel.records.len(), 2);

        panel.clear();
        assert!(panel.records.is_empty());
        assert_eq!(panel.selected, 0);
    }

    /// finish_record marks the correct record.
    #[test]
    fn test_finish_record_marks_done() {
        let mut panel = ToolOutputPanel::new();
        panel.push_record("grep");
        panel.finish_record("grep", true);
        assert_eq!(panel.records[0].status, ToolStatus::Done);
    }

    /// finish_record with success=false marks error.
    #[test]
    fn test_finish_record_marks_error() {
        let mut panel = ToolOutputPanel::new();
        panel.push_record("shell");
        panel.finish_record("shell", false);
        assert_eq!(panel.records[0].status, ToolStatus::Error);
    }
}
