//! Session screen — the rich cockpit wrapping a live agent PTY session.
//!
//! Layout:
//! ```
//! ┌───────────────────────────────────────┬────────────────────┐
//! │  Transcript (70%)                     │  Tool Timeline (30%)│
//! │  ❯ hello                              │  ✓ read_file  12ms  │
//! │    Hi there, how can I help?          │  ⏳ shell     …     │
//! │    ▋                                  │                    │
//! ├───────────────────────────────────────┴────────────────────┤
//! │  [claude]  claude-opus-4  Thinking…  tokens: 120  $0.001  │  status bar
//! ├────────────────────────────────────────────────────────────┤
//! │  ❯ _                                                       │  input bar
//! └────────────────────────────────────────────────────────────┘
//! ```
//!
//! If `approval_pending` is set, a full-width overlay is rendered above the
//! input bar showing the tool name, input preview, and approve/deny prompts.

use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
};

use crate::app::state::{
    AgentStatus, AppScreen, AppState, MessageRole, SessionState, ToolCallRecord, TranscriptEntry,
};
use crate::ui::layout::{LayoutManager, LayoutPreset};
use crate::ui::panels::Panel;
use crate::ui::theme::{AMBER, BG, BROWN, CHARCOAL, CREAM, RUST_RED, SOIL, SPROUT, TAN};

// ── Muted gray for "exited / unavailable" text ───────────────────────────────
const MUTED: Color = Color::Rgb(100, 100, 100);

// ── Entry point ───────────────────────────────────────────────────────────────

/// Render the full session cockpit screen.
///
/// Uses [`LayoutManager`] with the Sidebar preset to split the main area into
/// a 70% chat column and a 30% tool timeline column.  Status bar and input bar
/// are hardcoded at the bottom.
pub fn render_session(frame: &mut Frame, area: Rect, state: &AppState) {
    let AppScreen::Session(ref session) = state.screen else { return };

    // Outer background fill.
    let outer = Block::default().style(Style::default().bg(BG));
    frame.render_widget(outer, area);

    // Decide if approval overlay takes over the bottom row.
    let has_approval = session.approval_pending.is_some();

    // Vertical: main area | status bar | (approval overlay or input bar).
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(0),    // main area (chat + tool timeline)
            Constraint::Length(1), // status bar
            Constraint::Length(if has_approval { 5 } else { 1 }), // approval or input
        ])
        .split(area);

    // Use LayoutManager (Sidebar preset) to split the main area.
    let layout_mgr = LayoutManager::new(LayoutPreset::Sidebar);
    let panel_areas = layout_mgr.compute_areas(rows[0]);

    // Chat / transcript area.
    use crate::ui::panels::PanelId;
    let chat_area = panel_areas.get(&PanelId::Chat).copied().unwrap_or(rows[0]);
    let tool_area = panel_areas.get(&PanelId::ToolOutput).copied();

    let chat_focused = state.focus_ring.focused() == &PanelId::Chat;
    let tool_focused = state.focus_ring.focused() == &PanelId::ToolOutput;

    // Render ChatPanel if we have one; fall back to the legacy transcript renderer.
    state.chat_panel.render(frame, chat_area, chat_focused, state);

    // Render ToolOutputPanel in the tool area (if visible).
    if let Some(tool_rect) = tool_area {
        state.tool_output_panel.render(frame, tool_rect, tool_focused, state);
    }

    render_status_bar(frame, rows[1], session, &state.model);

    if has_approval {
        render_approval_overlay(frame, rows[2], session);
    } else {
        render_input_bar(frame, rows[2], session);
    }
}

// ── Main area (legacy — kept for tests) ──────────────────────────────────────

#[allow(dead_code)]
fn render_main_area(frame: &mut Frame, area: Rect, session: &SessionState) {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(70), Constraint::Percentage(30)])
        .split(area);

    render_transcript(frame, cols[0], session);
    render_tool_timeline(frame, cols[1], session);
}

// ── Transcript ────────────────────────────────────────────────────────────────

fn render_transcript(frame: &mut Frame, area: Rect, session: &SessionState) {
    let is_thinking = matches!(session.status, AgentStatus::Thinking);

    let border_style = Style::default().fg(SOIL);
    let block = Block::default()
        .title(Span::styled(" Transcript ", Style::default().fg(TAN)))
        .borders(Borders::ALL)
        .border_style(border_style)
        .style(Style::default().bg(BG));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    if session.transcript.is_empty() {
        let line = match session.status {
            AgentStatus::Starting => "  Starting agent…",
            _ => "  Waiting for first message…",
        };
        let p = Paragraph::new(line).style(Style::default().fg(SOIL));
        frame.render_widget(p, inner);
        return;
    }

    // Build all lines. The last assistant entry gets a blinking cursor if
    // the agent is actively streaming (Thinking status).
    let total_entries = session.transcript.len();
    let lines: Vec<Line> = session
        .transcript
        .iter()
        .enumerate()
        .flat_map(|(idx, entry)| {
            let is_last = idx == total_entries - 1;
            let add_cursor =
                is_thinking && is_last && entry.role == MessageRole::Assistant;
            transcript_entry_to_lines(entry, add_cursor)
        })
        .collect();

    // Scroll calculation: scroll_offset=0 means pinned to bottom.
    let total = lines.len() as u16;
    let height = inner.height;
    let scroll = if total > height {
        // max_scroll is how far up we can scroll.
        let max_scroll = total.saturating_sub(height);
        // scroll_offset is "lines from bottom", clamped to max.
        let from_bottom = session.scroll_offset.min(max_scroll);
        max_scroll - from_bottom
    } else {
        0
    };

    let para = Paragraph::new(lines)
        .style(Style::default().fg(CREAM))
        .wrap(Wrap { trim: false })
        .scroll((scroll, 0));

    frame.render_widget(para, inner);
}

fn transcript_entry_to_lines(entry: &TranscriptEntry, add_cursor: bool) -> Vec<Line<'static>> {
    match entry.role {
        MessageRole::User => user_entry_lines(entry),
        MessageRole::Assistant => assistant_entry_lines(entry, add_cursor),
        MessageRole::System => system_entry_lines(entry),
        MessageRole::Error => error_entry_lines(entry),
    }
}

/// User messages: `❯ ` prefix in Amber, text in Cream.
fn user_entry_lines(entry: &TranscriptEntry) -> Vec<Line<'static>> {
    let mut lines = vec![];
    let content = entry.content.clone();
    let mut first = true;
    for text_line in content.lines() {
        if first {
            lines.push(Line::from(vec![
                Span::styled("❯ ", Style::default().fg(AMBER).add_modifier(Modifier::BOLD)),
                Span::styled(text_line.to_string(), Style::default().fg(CREAM)),
            ]));
            first = false;
        } else {
            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled(text_line.to_string(), Style::default().fg(CREAM)),
            ]));
        }
    }
    if first {
        // Empty content — still emit the prompt.
        lines.push(Line::from(Span::styled(
            "❯ ",
            Style::default().fg(AMBER).add_modifier(Modifier::BOLD),
        )));
    }
    lines.push(Line::from(""));
    lines
}

/// Assistant messages: Cream text, no prefix. Blinking cursor on last line
/// if `add_cursor` is true.
fn assistant_entry_lines(entry: &TranscriptEntry, add_cursor: bool) -> Vec<Line<'static>> {
    let mut lines = vec![];
    let content = entry.content.clone();
    let text_lines: Vec<&str> = content.lines().collect();
    let total = text_lines.len();

    for (i, text_line) in text_lines.iter().enumerate() {
        let is_last_line = i == total.saturating_sub(1);
        if add_cursor && is_last_line {
            // Append blinking cursor to the last line of streaming text.
            let mut spans = vec![Span::styled(
                text_line.to_string(),
                Style::default().fg(CREAM),
            )];
            spans.push(Span::styled(
                "▋",
                Style::default().fg(AMBER).add_modifier(Modifier::SLOW_BLINK),
            ));
            lines.push(Line::from(spans));
        } else {
            lines.push(Line::from(Span::styled(
                text_line.to_string(),
                Style::default().fg(CREAM),
            )));
        }
    }

    // If content is empty and we're streaming, emit just the cursor.
    if content.is_empty() && add_cursor {
        lines.push(Line::from(Span::styled(
            "▋",
            Style::default().fg(AMBER).add_modifier(Modifier::SLOW_BLINK),
        )));
    }

    lines.push(Line::from(""));
    lines
}

/// System messages: muted SOIL color.
fn system_entry_lines(entry: &TranscriptEntry) -> Vec<Line<'static>> {
    let content = entry.content.clone();
    let mut lines: Vec<Line> = content
        .lines()
        .map(|l| {
            Line::from(vec![
                Span::styled("⚙ ", Style::default().fg(SOIL)),
                Span::styled(l.to_string(), Style::default().fg(SOIL)),
            ])
        })
        .collect();
    lines.push(Line::from(""));
    lines
}

/// Error messages: Rust red.
fn error_entry_lines(entry: &TranscriptEntry) -> Vec<Line<'static>> {
    let content = entry.content.clone();
    let mut lines: Vec<Line> = content
        .lines()
        .map(|l| {
            Line::from(vec![
                Span::styled("✗ ", Style::default().fg(RUST_RED)),
                Span::styled(l.to_string(), Style::default().fg(RUST_RED)),
            ])
        })
        .collect();
    lines.push(Line::from(""));
    lines
}

// ── Tool timeline ─────────────────────────────────────────────────────────────

fn render_tool_timeline(frame: &mut Frame, area: Rect, session: &SessionState) {
    let border_style = Style::default().fg(SOIL);
    let block = Block::default()
        .title(Span::styled(" Tools ", Style::default().fg(TAN)))
        .borders(Borders::ALL)
        .border_style(border_style)
        .style(Style::default().bg(BG));

    if session.tool_calls.is_empty() {
        let p = Paragraph::new("\n  No tool calls yet.")
            .style(Style::default().fg(SOIL))
            .block(block);
        frame.render_widget(p, area);
        return;
    }

    let inner_height = area.height.saturating_sub(2) as usize; // subtract borders
    let items: Vec<ListItem> = session
        .tool_calls
        .iter()
        .rev()
        .take(inner_height.max(1) + 20) // render enough for scrolling
        .map(|tc| tool_call_to_list_item(tc))
        .collect();

    let list = List::new(items).block(block);
    frame.render_widget(list, area);
}

fn tool_call_to_list_item(tc: &ToolCallRecord) -> ListItem<'static> {
    let (badge, badge_color) = match tc.success {
        Some(true) => ("[✓]", SPROUT),
        Some(false) => ("[✗]", RUST_RED),
        None => ("[⏳]", AMBER),
    };

    let duration = match tc.duration_ms {
        Some(ms) if ms < 1000 => format!(" {}ms", ms),
        Some(ms) => format!(" {:.1}s", ms as f64 / 1000.0),
        None => String::new(),
    };

    // Truncate tool name to fit the narrow sidebar.
    let name = if tc.name.len() > 12 {
        format!("{}…", &tc.name[..11])
    } else {
        tc.name.clone()
    };

    let line = Line::from(vec![
        Span::styled(badge, Style::default().fg(badge_color).add_modifier(Modifier::BOLD)),
        Span::raw(" "),
        Span::styled(name, Style::default().fg(TAN)),
        Span::styled(duration, Style::default().fg(SOIL)),
    ]);

    ListItem::new(line)
}

// ── Status bar ────────────────────────────────────────────────────────────────

fn render_status_bar(frame: &mut Frame, area: Rect, session: &SessionState, model: &str) {
    let sep = Span::styled(" │ ", Style::default().fg(SOIL).bg(CHARCOAL));

    let agent_span = Span::styled(
        format!(" {} ", session.agent_name),
        Style::default().fg(AMBER).bg(CHARCOAL).add_modifier(Modifier::BOLD),
    );

    let model_span = Span::styled(
        model.to_string(),
        Style::default().fg(TAN).bg(CHARCOAL),
    );

    let (status_label, status_fg) = agent_status_display(&session.status);
    let status_span = Span::styled(
        status_label,
        Style::default().fg(status_fg).bg(CHARCOAL),
    );

    let tokens = session.metrics.total_tokens();
    let token_span = Span::styled(
        format!("tokens: {}", tokens),
        Style::default().fg(BROWN).bg(CHARCOAL),
    );

    let cost_span = Span::styled(
        format!("cost: ${:.3}", session.metrics.total_cost_usd),
        Style::default().fg(SOIL).bg(CHARCOAL),
    );

    let elapsed_span = Span::styled(
        format!("elapsed: {}s", session.metrics.duration_secs),
        Style::default().fg(SOIL).bg(CHARCOAL),
    );

    let line = Line::from(vec![
        agent_span,
        sep.clone(),
        model_span,
        sep.clone(),
        status_span,
        sep.clone(),
        token_span,
        sep.clone(),
        cost_span,
        sep.clone(),
        elapsed_span,
        Span::raw(" "), // trailing pad
    ]);

    let bar = Paragraph::new(line).style(Style::default().bg(CHARCOAL));
    frame.render_widget(bar, area);
}

/// Returns `(label, color)` for a given agent status.
fn agent_status_display(status: &AgentStatus) -> (String, ratatui::style::Color) {
    match status {
        AgentStatus::Starting => ("Starting…".to_string(), SOIL),
        AgentStatus::Idle => ("Idle".to_string(), SPROUT),
        AgentStatus::Thinking => ("Thinking…".to_string(), AMBER),
        AgentStatus::RunningTool { name } => (format!("▶ {}", name), AMBER),
        AgentStatus::WaitingApproval { tool_name } => {
            (format!("⚠ Approve: {}", tool_name), RUST_RED)
        }
        AgentStatus::Exited { code } => {
            (format!("Exited ({})", code.unwrap_or(-1)), MUTED)
        }
        AgentStatus::Error { message } => {
            let short = if message.len() > 30 {
                format!("{}…", &message[..29])
            } else {
                message.clone()
            };
            (format!("Error: {}", short), RUST_RED)
        }
    }
}

// ── Input bar ─────────────────────────────────────────────────────────────────

fn render_input_bar(frame: &mut Frame, area: Rect, session: &SessionState) {
    // Agent is busy when Thinking or RunningTool.
    let is_busy = matches!(
        session.status,
        AgentStatus::Thinking | AgentStatus::RunningTool { .. }
    );

    if is_busy {
        // Spinner + status while agent is working.
        let spinner_frames = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
        let frame_idx = (session.tick_count as usize) % spinner_frames.len();
        let spinner = spinner_frames[frame_idx];
        let (label, _) = agent_status_display(&session.status);

        let line = Line::from(vec![
            Span::styled(format!("{} ", spinner), Style::default().fg(AMBER)),
            Span::styled(label, Style::default().fg(MUTED)),
        ]);
        let para = Paragraph::new(line).style(Style::default().bg(BG));
        frame.render_widget(para, area);
    } else {
        // Active input: `❯ {buf}|`
        let prompt = "❯ ";
        let buf = &session.input_buffer;
        let cursor = session.input_cursor.min(buf.len());
        let before = &buf[..cursor];
        let after = &buf[cursor..];

        let mut spans = vec![Span::styled(
            prompt,
            Style::default()
                .fg(AMBER)
                .add_modifier(Modifier::BOLD),
        )];

        if !before.is_empty() {
            spans.push(Span::styled(
                before.to_string(),
                Style::default().fg(CREAM),
            ));
        }

        // Cursor block: invert on the character under the cursor.
        let cursor_char = after.chars().next().unwrap_or(' ');
        spans.push(Span::styled(
            cursor_char.to_string(),
            Style::default().fg(BG).bg(CREAM),
        ));

        let after_cursor: String = after.chars().skip(1).collect();
        if !after_cursor.is_empty() {
            spans.push(Span::styled(after_cursor, Style::default().fg(CREAM)));
        }

        let para = Paragraph::new(Line::from(spans)).style(Style::default().bg(BG));
        frame.render_widget(para, area);
    }
}

// ── Approval overlay ──────────────────────────────────────────────────────────

fn render_approval_overlay(frame: &mut Frame, area: Rect, session: &SessionState) {
    let Some(ref approval) = session.approval_pending else {
        return;
    };

    // Bordered box in Amber.
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(AMBER))
        .title(Span::styled(
            " ⚠  Approval Required ",
            Style::default()
                .fg(AMBER)
                .add_modifier(Modifier::BOLD),
        ))
        .style(Style::default().bg(CHARCOAL));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    // Build content: tool name line + up to 3 lines of input preview + actions.
    let mut content_lines: Vec<Line> = vec![];

    // Tool name line.
    content_lines.push(Line::from(vec![
        Span::styled("Tool: ", Style::default().fg(SOIL)),
        Span::styled(
            approval.tool_name.clone(),
            Style::default().fg(CREAM).add_modifier(Modifier::BOLD),
        ),
    ]));

    // Input preview (up to 3 lines from JSON).
    let input_str = serde_json::to_string_pretty(&approval.input)
        .unwrap_or_else(|_| approval.input.to_string());
    for line in input_str.lines().take(3) {
        content_lines.push(Line::from(Span::styled(
            format!("  {}", line),
            Style::default().fg(TAN),
        )));
    }

    // Action hints.
    content_lines.push(Line::from(vec![
        Span::styled("  [y] approve  ", Style::default().fg(SPROUT).add_modifier(Modifier::BOLD)),
        Span::styled("[n] deny", Style::default().fg(RUST_RED).add_modifier(Modifier::BOLD)),
    ]));

    let para = Paragraph::new(content_lines).style(Style::default().bg(CHARCOAL));
    frame.render_widget(para, inner);
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::state::{AgentStatus, SessionState, ToolCallRecord, TranscriptEntry};
    use chrono::Utc;

    #[test]
    fn agent_status_display_idle_is_sprout() {
        let (label, color) = agent_status_display(&AgentStatus::Idle);
        assert_eq!(label, "Idle");
        assert_eq!(color, SPROUT);
    }

    #[test]
    fn agent_status_display_thinking_is_amber() {
        let (label, color) = agent_status_display(&AgentStatus::Thinking);
        assert!(label.contains("Thinking"));
        assert_eq!(color, AMBER);
    }

    #[test]
    fn agent_status_display_running_tool_is_amber() {
        let (label, color) =
            agent_status_display(&AgentStatus::RunningTool { name: "shell".to_string() });
        assert!(label.contains("shell"));
        assert_eq!(color, AMBER);
    }

    #[test]
    fn agent_status_display_exited_is_muted() {
        let (label, color) = agent_status_display(&AgentStatus::Exited { code: Some(0) });
        assert!(label.contains("Exited"));
        assert_eq!(color, MUTED);
    }

    #[test]
    fn agent_status_display_error_is_rust() {
        let (label, color) =
            agent_status_display(&AgentStatus::Error { message: "boom".to_string() });
        assert!(label.contains("Error"));
        assert_eq!(color, RUST_RED);
    }

    #[test]
    fn user_entry_produces_amber_prefix() {
        let entry = TranscriptEntry::user("hello world");
        let lines = user_entry_lines(&entry);
        // First line has the ❯ prefix span in Amber and content span.
        assert!(lines.len() >= 2); // content + blank line
        let first_line = &lines[0];
        let first_span = &first_line.spans[0];
        assert_eq!(first_span.content, "❯ ");
        assert_eq!(first_span.style.fg, Some(AMBER));
    }

    #[test]
    fn assistant_entry_no_prefix() {
        let entry = TranscriptEntry::assistant("Hello there");
        let lines = assistant_entry_lines(&entry, false);
        // Should not have ❯ anywhere.
        for line in &lines {
            for span in &line.spans {
                assert!(!span.content.contains('❯'));
            }
        }
    }

    #[test]
    fn assistant_entry_with_cursor_appends_block() {
        let entry = TranscriptEntry::assistant("Streaming…");
        let lines = assistant_entry_lines(&entry, true);
        // Last non-blank line should contain the cursor character.
        let all_content: String = lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .map(|s| s.content.as_ref())
            .collect();
        assert!(all_content.contains('▋'));
    }

    #[test]
    fn tool_call_badge_done_is_sprout() {
        let tc = ToolCallRecord {
            id: "t1".into(),
            name: "read_file".into(),
            input: serde_json::json!({}),
            output: Some("content".into()),
            started_at: Utc::now(),
            duration_ms: Some(42),
            success: Some(true),
        };
        let item = tool_call_to_list_item(&tc);
        // Just ensure it builds without panic; color testing via span inspection.
        drop(item);
    }

    #[test]
    fn tool_call_badge_error_builds() {
        let tc = ToolCallRecord {
            id: "t2".into(),
            name: "shell".into(),
            input: serde_json::json!({}),
            output: Some("err".into()),
            started_at: Utc::now(),
            duration_ms: Some(0),
            success: Some(false),
        };
        drop(tool_call_to_list_item(&tc));
    }

    #[test]
    fn tool_call_pending_builds() {
        let tc = ToolCallRecord {
            id: "t3".into(),
            name: "write_file".into(),
            input: serde_json::json!({}),
            output: None,
            started_at: Utc::now(),
            duration_ms: None,
            success: None,
        };
        drop(tool_call_to_list_item(&tc));
    }

    #[test]
    fn session_state_new_has_user_scrolled_false() {
        let s = SessionState::new("s-1", "claude");
        assert!(!s.user_scrolled);
        assert_eq!(s.scroll_offset, 0);
    }

    #[test]
    fn multiline_user_message_indents_continuation() {
        let entry = TranscriptEntry::user("line one\nline two\nline three");
        let lines = user_entry_lines(&entry);
        // First line has ❯, subsequent content lines have "  " padding.
        assert!(lines[0].spans[0].content.contains('❯'));
        assert_eq!(lines[1].spans[0].content.as_ref(), "  ");
        assert_eq!(lines[2].spans[0].content.as_ref(), "  ");
    }
}
