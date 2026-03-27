//! User interface — layout, theme, panels, widgets, and overlays.

pub mod layout;
pub mod overlays;
pub mod panels;
pub mod theme;
pub mod widgets;

use ratatui::{
    Frame,
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Widget},
};

use crate::agent::state_machine::AgentState;
use crate::app::state::AppState;
use crate::ui::layout::build_layout;
use crate::ui::panels::chat::render_chat;
use crate::ui::theme::{Theme, AMBER, BG, BROWN, CHARCOAL, CREAM, RUST_RED, SOIL, TAN};
use crate::ui::widgets::{
    approval_bar::ApprovalBar,
    status_badge::{BadgeVariant, StatusBadge},
};

// ── Top-level view function ───────────────────────────────────────────────────

/// Draw the entire Potato UI for one frame.
///
/// This is the single entry point called from the main event loop.
pub fn view(frame: &mut Frame, state: &AppState) {
    let theme = Theme::default();
    let areas = build_layout(frame.area(), state);

    // 1. Chat / conversation panel
    render_chat(frame, areas.chat, state, &theme);

    // 2. Input area — or approval bar if waiting for approval
    if let Some(ref approval) = state.pending_approval {
        // Render the approval bar instead of the text input
        let bar = ApprovalBar::new(approval, &theme);
        frame.render_widget(bar, areas.input);
    } else {
        render_input(frame, areas.input, state, &theme);
    }

    // 3. Status bar
    render_status_bar(frame, areas.status_bar, state, &theme);
}

// ── Input area ────────────────────────────────────────────────────────────────

fn render_input(frame: &mut Frame, area: ratatui::layout::Rect, state: &AppState, theme: &Theme) {
    use ratatui::style::Modifier;

    let is_busy = state.agent_state != AgentState::Idle;

    let border_style = if is_busy {
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

    if is_busy {
        // Show a spinner / waiting message
        let spinner_frames = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
        let frame_idx = (state.tick_count as usize) % spinner_frames.len();
        let spinner = spinner_frames[frame_idx];

        let state_label = agent_state_label(&state.agent_state);
        let line = Line::from(vec![
            Span::styled(format!("{} ", spinner), Style::default().fg(AMBER)),
            Span::styled(state_label, Style::default().fg(SOIL)),
        ]);
        Paragraph::new(line).render(inner, frame.buffer_mut());
    } else {
        // Show the text input with cursor
        let prompt = "❯ ";
        let prompt_style = theme.input_prompt();
        let text_style = theme.input_active();

        // Build content: text before cursor, cursor char, text after cursor
        let buf = &state.input_buffer;
        let cursor = state.input_cursor;

        let before = &buf[..cursor];
        let after = if cursor < buf.len() {
            &buf[cursor..]
        } else {
            ""
        };

        let mut spans = vec![Span::styled(prompt.to_string(), prompt_style)];

        if !before.is_empty() {
            spans.push(Span::styled(before.to_string(), text_style));
        }

        // Cursor block
        let cursor_char = after.chars().next().unwrap_or(' ');
        spans.push(Span::styled(
            cursor_char.to_string(),
            Style::default()
                .fg(BG)
                .bg(CREAM),
        ));

        // Text after cursor (skip the cursor char)
        let after_cursor: String = after.chars().skip(1).collect();
        if !after_cursor.is_empty() {
            spans.push(Span::styled(after_cursor, text_style));
        }

        let line = Line::from(spans);
        Paragraph::new(line).render(inner, frame.buffer_mut());
    }
}

// ── Status bar ────────────────────────────────────────────────────────────────

fn render_status_bar(
    frame: &mut Frame,
    area: ratatui::layout::Rect,
    state: &AppState,
    theme: &Theme,
) {
    let sep = Span::styled(" │ ", theme.status_separator());
    let _base = theme.status_bar();

    // Model name
    let model_span = Span::styled(
        format!(" {}", state.model),
        Style::default().fg(TAN).bg(CHARCOAL),
    );

    // Agent state badge
    let (state_label, state_style) = agent_state_display(&state.agent_state, theme);
    let state_span = Span::styled(state_label, state_style.bg(CHARCOAL));

    // Token count
    let tokens = state.token_counts.0 + state.token_counts.1;
    let token_span = Span::styled(
        format!("{} tok", tokens),
        Style::default().fg(BROWN).bg(CHARCOAL),
    );

    // Error message overrides the right side if present
    let right_span = if let Some(ref err) = state.error_message {
        Span::styled(
            format!("⚠ {}", err),
            Style::default().fg(RUST_RED).bg(CHARCOAL),
        )
    } else {
        // Session / config info
        let config = if state.config_path.is_empty() {
            "default config".to_string()
        } else {
            // Show just the filename
            std::path::Path::new(&state.config_path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(&state.config_path)
                .to_string()
        };
        Span::styled(
            format!("{} ", config),
            Style::default().fg(SOIL).bg(CHARCOAL),
        )
    };

    let line = Line::from(vec![
        model_span,
        sep.clone(),
        state_span,
        sep.clone(),
        token_span,
        sep,
        right_span,
    ]);

    Paragraph::new(line)
        .style(Style::default().bg(CHARCOAL))
        .render(area, frame.buffer_mut());
}

// ── Agent state helpers ───────────────────────────────────────────────────────

/// Short human-readable label for the current agent state.
fn agent_state_label(state: &AgentState) -> String {
    match state {
        AgentState::Idle => "Idle".to_string(),
        AgentState::Thinking => "Thinking…".to_string(),
        AgentState::ToolCall { tool_name } => format!("Running {}", tool_name),
        AgentState::Approval { tool_name, .. } => format!("Approval: {}", tool_name),
        AgentState::Error(_) => "Error".to_string(),
    }
}

/// Return a (label, Style) pair for the agent state badge.
fn agent_state_display(state: &AgentState, _theme: &Theme) -> (String, Style) {
    match state {
        AgentState::Idle => ("Idle".to_string(), Style::default().fg(TAN)),
        AgentState::Thinking => (
            "Thinking…".to_string(),
            Style::default().fg(AMBER),
        ),
        AgentState::ToolCall { tool_name } => (
            format!("● {}", tool_name),
            Style::default().fg(AMBER),
        ),
        AgentState::Approval { tool_name, .. } => (
            format!("⚠ {}", tool_name),
            Style::default().fg(AMBER),
        ),
        AgentState::Error(_) => ("Error".to_string(), Style::default().fg(RUST_RED)),
    }
}
