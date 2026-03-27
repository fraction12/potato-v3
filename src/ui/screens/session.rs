//! Session screen — the rich cockpit wrapping a live agent PTY session.
//!
//! Layout:
//! ```
//! ┌──────────────────────────────────┬────────────────────┐
//! │  Transcript                      │  Tool Timeline     │
//! │  ▶ User: hello                   │  ● read_file 12ms  │
//! │  ◀ Assistant: hi there           │  ● shell     200ms │
//! │                                  │                    │
//! ├──────────────────────────────────┴────────────────────┤
//! │  [claude]  claude-opus-4  Thinking…  120tok $0.001    │  status bar
//! ├────────────────────────────────────────────────────────┤
//! │  ❯ _                                                   │  input bar
//! └────────────────────────────────────────────────────────┘
//! ```
//!
//! If `approval_pending` is set, a full-width overlay is rendered over the input bar.

use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
};

use crate::app::state::{
    AgentStatus, AppScreen, AppState, MessageRole, SessionState, ToolCallRecord, TranscriptEntry,
};
use crate::ui::theme::{AMBER, BG, BROWN, CHARCOAL, CREAM, RUST_RED, SOIL, TAN};

// ── Entry point ───────────────────────────────────────────────────────────────

/// Render the full session cockpit screen.
pub fn render_session(frame: &mut Frame, area: Rect, state: &AppState) {
    let AppScreen::Session(ref session) = state.screen else { return };

    // Outer fill.
    let outer = Block::default().style(Style::default().bg(BG));
    frame.render_widget(outer, area);

    // Vertical: main area | status bar | input bar.
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(0),     // main area (transcript + tool timeline)
            Constraint::Length(1),  // status bar
            Constraint::Length(3),  // input bar or approval overlay
        ])
        .split(area);

    render_main_area(frame, rows[0], session);
    render_status_bar(frame, rows[1], session, &state.model);

    if session.approval_pending.is_some() {
        render_approval_overlay(frame, rows[2], session);
    } else {
        render_input_bar(frame, rows[2], session);
    }
}

// ── Main area ─────────────────────────────────────────────────────────────────

fn render_main_area(frame: &mut Frame, area: Rect, session: &SessionState) {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(65), Constraint::Percentage(35)])
        .split(area);

    render_transcript(frame, cols[0], session);
    render_tool_timeline(frame, cols[1], session);
}

// ── Transcript ────────────────────────────────────────────────────────────────

fn render_transcript(frame: &mut Frame, area: Rect, session: &SessionState) {
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

    let lines: Vec<Line> = session
        .transcript
        .iter()
        .flat_map(|entry| transcript_entry_to_lines(entry))
        .collect();

    let total = lines.len() as u16;
    let height = inner.height;
    let scroll = if total > height {
        let max_scroll = total - height;
        max_scroll.saturating_sub(session.scroll_offset)
    } else {
        0
    };

    let para = Paragraph::new(lines)
        .style(Style::default().fg(CREAM))
        .wrap(Wrap { trim: false })
        .scroll((scroll, 0));

    frame.render_widget(para, inner);
}

fn transcript_entry_to_lines(entry: &TranscriptEntry) -> Vec<Line<'static>> {
    let (prefix, fg) = match entry.role {
        MessageRole::User => ("▶ You: ", AMBER),
        MessageRole::Assistant => ("◀ Agent: ", CREAM),
        MessageRole::System => ("⚙ System: ", SOIL),
        MessageRole::Error => ("✗ Error: ", RUST_RED),
    };

    let ts = entry.timestamp.format("%H:%M").to_string();

    let mut lines = vec![];
    lines.push(Line::from(vec![
        Span::styled(format!("[{}] ", ts), Style::default().fg(SOIL)),
        Span::styled(prefix, Style::default().fg(fg).add_modifier(Modifier::BOLD)),
    ]));

    for text_line in entry.content.lines() {
        lines.push(Line::from(Span::styled(
            format!("  {}", text_line),
            Style::default().fg(fg),
        )));
    }
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
        let p = Paragraph::new("  No tool calls yet.")
            .style(Style::default().fg(SOIL))
            .block(block);
        frame.render_widget(p, area);
        return;
    }

    let items: Vec<ListItem> = session
        .tool_calls
        .iter()
        .rev()
        .take(50)
        .map(|tc| tool_call_to_list_item(tc))
        .collect();

    let list = List::new(items).block(block);
    frame.render_widget(list, area);
}

fn tool_call_to_list_item(tc: &ToolCallRecord) -> ListItem<'static> {
    let (indicator, colour) = match tc.success {
        Some(true) => ("✓", AMBER),
        Some(false) => ("✗", RUST_RED),
        None => ("◌", BROWN),
    };

    let duration = tc.duration_ms.map_or("…".to_string(), |ms| format!("{}ms", ms));

    let line = Line::from(vec![
        Span::styled(format!(" {} ", indicator), Style::default().fg(colour)),
        Span::styled(tc.name.clone(), Style::default().fg(TAN)),
        Span::styled(format!(" {}", duration), Style::default().fg(SOIL)),
    ]);

    ListItem::new(line)
}

// ── Status bar ────────────────────────────────────────────────────────────────

fn render_status_bar(frame: &mut Frame, area: Rect, session: &SessionState, model: &str) {
    let sep = Span::styled(" │ ", Style::default().fg(SOIL));

    let agent_span = Span::styled(
        format!(" [{}]", session.agent_name),
        Style::default().fg(AMBER).bg(CHARCOAL),
    );

    let model_span = Span::styled(
        format!(" {}", model),
        Style::default().fg(TAN).bg(CHARCOAL),
    );

    let (status_label, status_fg) = agent_status_display(&session.status);
    let status_span = Span::styled(status_label, Style::default().fg(status_fg).bg(CHARCOAL));

    let tokens = session.metrics.total_tokens();
    let token_span = Span::styled(
        format!(" {}tok", tokens),
        Style::default().fg(BROWN).bg(CHARCOAL),
    );

    let cost_span = Span::styled(
        format!(" ${:.4}", session.metrics.total_cost_usd),
        Style::default().fg(SOIL).bg(CHARCOAL),
    );

    let elapsed_span = Span::styled(
        format!(" {}s ", session.metrics.duration_secs),
        Style::default().fg(SOIL).bg(CHARCOAL),
    );

    let line = Line::from(vec![
        agent_span, sep.clone(),
        model_span, sep.clone(),
        status_span, sep.clone(),
        token_span, sep.clone(),
        cost_span, sep.clone(),
        elapsed_span,
    ]);

    let bar = Paragraph::new(line).style(Style::default().bg(CHARCOAL));
    frame.render_widget(bar, area);
}

fn agent_status_display(status: &AgentStatus) -> (String, ratatui::style::Color) {
    match status {
        AgentStatus::Starting => ("Starting…".to_string(), SOIL),
        AgentStatus::Idle => ("Idle".to_string(), TAN),
        AgentStatus::Thinking => ("Thinking…".to_string(), AMBER),
        AgentStatus::RunningTool { name } => (format!("● {}", name), AMBER),
        AgentStatus::WaitingApproval { tool_name } => (format!("⚠ Approve: {}", tool_name), RUST_RED),
        AgentStatus::Exited { code } => (format!("Exited ({})", code.unwrap_or(-1)), SOIL),
        AgentStatus::Error { message } => (format!("Error: {}", message), RUST_RED),
    }
}

// ── Input bar ─────────────────────────────────────────────────────────────────

fn render_input_bar(frame: &mut Frame, area: Rect, session: &SessionState) {
    let is_active = session.status.is_active();

    let border_style = if is_active {
        Style::default().fg(SOIL)
    } else {
        Style::default().fg(BROWN)
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(border_style)
        .style(Style::default().bg(BG));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    if is_active {
        // Show a spinner.
        let spinner_frames = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
        let frame_idx = (session.tick_count as usize) % spinner_frames.len();
        let spinner = spinner_frames[frame_idx];
        let (label, _) = agent_status_display(&session.status);
        let line = Line::from(vec![
            Span::styled(format!("{} ", spinner), Style::default().fg(AMBER)),
            Span::styled(label, Style::default().fg(SOIL)),
        ]);
        frame.render_widget(Paragraph::new(line), inner);
    } else {
        let prompt = "❯ ";
        let buf = &session.input_buffer;
        let cursor = session.input_cursor;

        let before = &buf[..cursor.min(buf.len())];
        let after = &buf[cursor.min(buf.len())..];

        let mut spans = vec![
            Span::styled(prompt, Style::default().fg(AMBER).add_modifier(Modifier::BOLD)),
        ];

        if !before.is_empty() {
            spans.push(Span::styled(before.to_string(), Style::default().fg(CREAM)));
        }

        let cursor_char = after.chars().next().unwrap_or(' ');
        spans.push(Span::styled(
            cursor_char.to_string(),
            Style::default().fg(BG).bg(CREAM),
        ));

        let after_cursor: String = after.chars().skip(1).collect();
        if !after_cursor.is_empty() {
            spans.push(Span::styled(after_cursor, Style::default().fg(CREAM)));
        }

        frame.render_widget(Paragraph::new(Line::from(spans)), inner);
    }
}

// ── Approval overlay ──────────────────────────────────────────────────────────

fn render_approval_overlay(frame: &mut Frame, area: Rect, session: &SessionState) {
    let Some(ref approval) = session.approval_pending else { return };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(RUST_RED))
        .title(Span::styled(" ⚠ Approval Required ", Style::default().fg(RUST_RED).add_modifier(Modifier::BOLD)))
        .style(Style::default().bg(CHARCOAL));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let line = Line::from(vec![
        Span::styled("Tool: ", Style::default().fg(SOIL)),
        Span::styled(approval.tool_name.clone(), Style::default().fg(AMBER).add_modifier(Modifier::BOLD)),
        Span::styled("    [y] Approve  [n] Deny", Style::default().fg(TAN)),
    ]);
    frame.render_widget(Paragraph::new(line), inner);
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::state::{AgentStatus, SessionState, TranscriptEntry, ToolCallRecord};
    use chrono::Utc;

    #[test]
    fn agent_status_display_idle() {
        let (label, _) = agent_status_display(&AgentStatus::Idle);
        assert_eq!(label, "Idle");
    }

    #[test]
    fn agent_status_display_thinking() {
        let (label, _) = agent_status_display(&AgentStatus::Thinking);
        assert!(label.contains("Thinking"));
    }

    #[test]
    fn agent_status_display_running_tool() {
        let (label, _) = agent_status_display(&AgentStatus::RunningTool { name: "shell".to_string() });
        assert!(label.contains("shell"));
    }

    #[test]
    fn transcript_entry_user_lines() {
        let entry = TranscriptEntry::user("hello");
        let lines = transcript_entry_to_lines(&entry);
        // Should have header line, content line, and blank line.
        assert!(lines.len() >= 2);
    }

    #[test]
    fn transcript_entry_assistant_lines() {
        let entry = TranscriptEntry::assistant("Hi there\nSecond line");
        let lines = transcript_entry_to_lines(&entry);
        assert!(lines.len() >= 3);
    }

    #[test]
    fn tool_call_pending_shows_dots() {
        let tc = ToolCallRecord {
            id: "t1".into(),
            name: "read_file".into(),
            input: serde_json::json!({}),
            output: None,
            started_at: Utc::now(),
            duration_ms: None,
            success: None,
        };
        let item = tool_call_to_list_item(&tc);
        // Just verify it doesn't panic and produces an item.
        drop(item);
    }

    #[test]
    fn session_state_new() {
        let s = SessionState::new("s-1", "claude");
        assert_eq!(s.session_id, "s-1");
        assert!(s.transcript.is_empty());
        assert!(s.tool_calls.is_empty());
    }
}
