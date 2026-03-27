//! Session screen — cockpit layout wrapping a live agent PTY session.
//!
//! Layout (3-column):
//! ```
//! ┌─────────────────┬──────────────────────────────────┬─────────────────┐
//! │  Sessions       │                                  │  Claude metrics │
//! │  (left rail)    │   Claude PTY terminal viewport   │  / tools        │
//! │                 │   (center, fills available h)    │  / skills       │
//! │  ● session-1    │                                  │  / other        │
//! │                 ├──────────────────────────────────┤                 │
//! │                 │  ❯ Potato input bar              │                 │
//! └─────────────────┴──────────────────────────────────┴─────────────────┘
//! │  Status bar (full width, 1 line)                                     │
//! └──────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Focus model
//!
//! Default focus: **Input**.
//!
//! `Tab` cycles: Sessions → Input → Terminal → Sidebar → Sessions.
//! `Shift+Tab` reverses.
//! `Ctrl+J` jumps directly to Terminal.
//! `Esc` returns to Input.
//!
//! - **Input** focus: characters go into `session.input_buffer`; Enter sends
//!   the buffered text plus a real terminal carriage return to the PTY stdin.
//! - **Terminal** focus: *all* key events (except Ctrl+Q/Ctrl+\) are converted
//!   to raw byte sequences and written to the PTY stdin unchanged. This lets
//!   the user interact with Claude's native pickers / approvals / menus.
//! - **Sessions / Sidebar** focus: arrow keys navigate lists; Enter/Esc return
//!   to Input.

use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph},
};

use crate::app::state::{AgentStatus, AppScreen, AppState, CockpitFocus, SessionState};
use crate::claude_log::{ClaudeSidebarData, ClaudeToolStatus};
use crate::ui::theme::{AMBER, BG, BRASS, CHARCOAL, CREAM, ROSE, SPROUT, STONE, TAN};

// ── Constants ─────────────────────────────────────────────────────────────────

const MUTED: Color = Color::Rgb(100, 100, 100);
/// Width of the left and right rails (columns).
const RAIL_WIDTH: u16 = 18;

// ── Entry point ───────────────────────────────────────────────────────────────

/// Render the full session cockpit screen.
///
/// Takes `&mut AppState` so it can call `real_pty.resize()` every frame to
/// keep the PTY size in sync with the rendered output area.
pub fn render_session(frame: &mut Frame, area: Rect, state: &mut AppState) {
    let AppScreen::Session(_) = state.screen else { return };

    // Outer background fill.
    frame.render_widget(Block::default().style(Style::default().bg(BG)), area);

    // ── Outer vertical split: [content_rows] | [status_bar 1 line] ───────────
    let [content_area, status_area] = Layout::vertical([
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .areas(area);

    // ── Horizontal split: [left_rail] | [center_col] | [right_rail] ──────────
    let [left_area, center_area, right_area] = Layout::horizontal([
        Constraint::Length(RAIL_WIDTH),
        Constraint::Min(0),
        Constraint::Length(RAIL_WIDTH + 4), // sidebar slightly wider
    ])
    .areas(content_area);

    // ── Center column: [pty_output (min)] | [input_bar 3 lines] ──────────────
    let [pty_area, input_area] = Layout::vertical([
        Constraint::Min(0),
        Constraint::Length(3),
    ])
    .areas(center_area);

    // Pull focus out before borrowing state mutably for the PTY resize.
    let focus = state
        .session()
        .map(|s| s.cockpit_focus)
        .unwrap_or(CockpitFocus::Input);

    // Render PTY viewport (needs &mut state for resize).
    render_pty_viewport(frame, pty_area, state, focus);

    // Now borrow session immutably for the rest.
    let AppScreen::Session(ref session) = state.screen else { return };

    render_left_rail(frame, left_area, session, focus);
    render_input_bar(frame, input_area, session, focus);
    render_right_rail(frame, right_area, state, focus);
    render_status_bar(frame, status_area, session, &state.model, focus);
}

// ── Left rail — session list ──────────────────────────────────────────────────

fn render_left_rail(frame: &mut Frame, area: Rect, session: &SessionState, focus: CockpitFocus) {
    let focused = focus == CockpitFocus::Sessions;
    let border_style = if focused {
        Style::default().fg(AMBER)
    } else {
        Style::default().fg(BRASS)
    };
    let title_style = if focused {
        Style::default().fg(AMBER).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(TAN)
    };

    // Build session list items. Currently we only show the active session.
    let short_id: String = session.session_id.chars().take(10).collect();
    let items: Vec<ListItem> = vec![ListItem::new(Line::from(vec![
        Span::styled("● ", Style::default().fg(SPROUT)),
        Span::styled(short_id, Style::default().fg(CREAM)),
    ]))
    .style(Style::default().bg(if focused {
        Color::Rgb(45, 30, 20)
    } else {
        BG
    }))];

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(border_style)
                .title(Span::styled(" Sessions ", title_style)),
        )
        .style(Style::default().fg(STONE).bg(BG));

    frame.render_widget(list, area);
}

// ── Center — PTY viewport ─────────────────────────────────────────────────────

fn render_pty_viewport(frame: &mut Frame, area: Rect, state: &mut AppState, focus: CockpitFocus) {
    let focused = focus == CockpitFocus::Terminal;
    let border_style = if focused {
        Style::default().fg(AMBER)
    } else {
        Style::default().fg(BRASS)
    };
    let title_style = if focused {
        Style::default().fg(AMBER).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(TAN).add_modifier(Modifier::BOLD)
    };

    // Inner area available to the PTY (minus border).
    let inner_cols = area.width.saturating_sub(2);
    let inner_rows = area.height.saturating_sub(2);
    let desired_scroll = state.session().map(|s| s.terminal_scroll).unwrap_or(0);

    let mut synced_scroll = None;

    if let Some(ref pty) = state.real_pty {
        // Resize PTY every frame so it matches the exact output rect.
        let _ = pty.resize(inner_cols.max(1), inner_rows.max(1));
        let actual_scroll = pty.set_scrollback(desired_scroll);
        synced_scroll = Some(actual_scroll);

        let title = if actual_scroll > 0 {
            Span::styled(format!(" Claude ↑{} ", actual_scroll), title_style)
        } else {
            Span::styled(" Claude ", title_style)
        };

        if let Ok(parser) = pty.screen.try_lock() {
            use tui_term::widget::PseudoTerminal;
            let widget = PseudoTerminal::new(parser.screen()).block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(border_style)
                    .title(title.clone()),
            );
            frame.render_widget(widget, area);
        } else {
            // Parser locked by reader thread — show busy hint.
            let busy = Paragraph::new("…")
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_style(border_style)
                        .title(title),
                )
                .style(Style::default().fg(STONE));
            frame.render_widget(busy, area);
        }
    } else {
        // No active PTY — placeholder.
        let placeholder = Paragraph::new(
            "\n  No active session.\n  Select an agent on the dashboard and press Enter.",
        )
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(BRASS))
                .title(Span::styled(" Claude ", Style::default().fg(STONE))),
        )
        .style(Style::default().fg(STONE));
        frame.render_widget(placeholder, area);
    }

    if let Some(actual_scroll) = synced_scroll {
        if let Some(session) = state.session_mut() {
            session.terminal_scroll = actual_scroll;
        }
    }

    // Render "TERMINAL" focus indicator in top-right corner of the block when
    // terminal focus is active so the user can see the mode clearly.
    if focused && area.height > 2 && area.width > 14 {
        let hint = Span::styled(
            " [TERM] ",
            Style::default()
                .fg(BG)
                .bg(AMBER)
                .add_modifier(Modifier::BOLD),
        );
        let hint_line = Line::from(vec![hint]);
        let hint_area = Rect {
            x: area.x + area.width.saturating_sub(10),
            y: area.y,
            width: 9,
            height: 1,
        };
        frame.render_widget(Paragraph::new(hint_line), hint_area);
    }
}

// ── Center bottom — input bar ─────────────────────────────────────────────────

fn render_input_bar(frame: &mut Frame, area: Rect, session: &SessionState, focus: CockpitFocus) {
    let focused = focus == CockpitFocus::Input;

    let is_busy = matches!(
        session.status,
        AgentStatus::Thinking | AgentStatus::RunningTool { .. }
    );

    if is_busy {
        let spinner_frames = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
        let frame_idx = (session.tick_count as usize) % spinner_frames.len();
        let spinner = spinner_frames[frame_idx];
        let label = agent_status_label(&session.status);

        let line = Line::from(vec![
            Span::styled(format!("{} ", spinner), Style::default().fg(AMBER)),
            Span::styled(label, Style::default().fg(MUTED)),
        ]);
        let para = Paragraph::new(line)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(BRASS))
                    .title(" Input "),
            )
            .style(Style::default().bg(BG));
        frame.render_widget(para, area);
    } else {
        let border_style = if focused {
            Style::default().fg(AMBER)
        } else {
            Style::default().fg(BRASS)
        };
        let title_style = if focused {
            Style::default().fg(AMBER).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(STONE)
        };

        let prompt = "❯ ";
        let buf = &session.input_buffer;
        let cursor = session.input_cursor.min(buf.len());
        let before = &buf[..cursor];
        let after = &buf[cursor..];

        let mut spans = vec![Span::styled(
            prompt,
            Style::default().fg(AMBER).add_modifier(Modifier::BOLD),
        )];

        if !before.is_empty() {
            spans.push(Span::styled(before.to_string(), Style::default().fg(CREAM)));
        }

        // Block cursor on the character under the cursor position.
        let cursor_char = after.chars().next().unwrap_or(' ');
        spans.push(Span::styled(
            cursor_char.to_string(),
            if focused {
                Style::default().fg(BG).bg(CREAM)
            } else {
                Style::default().fg(MUTED)
            },
        ));

        let after_cursor: String = after.chars().skip(1).collect();
        if !after_cursor.is_empty() {
            spans.push(Span::styled(after_cursor, Style::default().fg(CREAM)));
        }

        let widget = Paragraph::new(Line::from(spans))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(border_style)
                    .title(Span::styled(" Input ", title_style)),
            )
            .style(Style::default().fg(CREAM).bg(BG));
        frame.render_widget(widget, area);
    }
}

// ── Right rail — metrics / tools / sidebar ────────────────────────────────────

fn render_right_rail(frame: &mut Frame, area: Rect, state: &AppState, focus: CockpitFocus) {
    let focused = focus == CockpitFocus::Sidebar;
    let title_color = if focused { AMBER } else { TAN };

    // Split sidebar vertically: Metrics | Tools | Quick
    let [metrics_area, tools_area, quick_area] = Layout::vertical([
        Constraint::Length(7),
        Constraint::Length(10),
        Constraint::Min(0),
    ])
    .areas(area);

    let sidebar = state
        .claude_log
        .as_ref()
        .map(|t| t.snapshot())
        .unwrap_or_default();

    // ── Metrics ───────────────────────────────────────────────────────────────
    let metrics_text = vec![
        Line::from(vec![
            Span::styled("Model ", Style::default().fg(BRASS)),
            Span::raw(sidebar.model.unwrap_or_else(|| "—".to_string())),
        ]),
        Line::from(vec![
            Span::styled("Turns ", Style::default().fg(BRASS)),
            Span::raw(format!("{}", sidebar.turns)),
        ]),
        Line::from(vec![
            Span::styled("I/O   ", Style::default().fg(BRASS)),
            Span::raw(format!("{} / {}", sidebar.usage.input_tokens, sidebar.usage.output_tokens)),
        ]),
        Line::from(vec![
            Span::styled("Cache ", Style::default().fg(BRASS)),
            Span::raw(format!("{} / {}", sidebar.usage.cache_read_input_tokens, sidebar.usage.cache_creation_input_tokens)),
        ]),
        Line::from(vec![
            Span::styled("Stop  ", Style::default().fg(BRASS)),
            Span::raw(sidebar.last_stop_reason.unwrap_or_else(|| "—".to_string())),
        ]),
    ];

    frame.render_widget(
        Paragraph::new(metrics_text).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(if focused { AMBER } else { BRASS }))
                .title(Span::styled(" Claude ", Style::default().fg(title_color))),
        ),
        metrics_area,
    );

    // ── Tools ─────────────────────────────────────────────────────────────────
    let tools_text: Vec<Line> = if sidebar.tools.is_empty() {
        vec![Line::from(Span::styled("  waiting for Claude log…", Style::default().fg(STONE)))]
    } else {
        sidebar
            .tools
            .iter()
            .rev()
            .take(6)
            .map(|e| {
                let icon = match e.status {
                    ClaudeToolStatus::Done => Span::styled("✓ ", Style::default().fg(SPROUT)),
                    ClaudeToolStatus::Error => Span::styled("✗ ", Style::default().fg(ROSE)),
                    ClaudeToolStatus::Running => Span::styled("⏳ ", Style::default().fg(AMBER)),
                };
                let max_name = (area.width.saturating_sub(5)) as usize;
                let name = if e.name.len() > max_name && max_name > 1 {
                    format!("{}…", &e.name[..max_name.saturating_sub(1)])
                } else {
                    e.name.clone()
                };
                Line::from(vec![icon, Span::styled(name, Style::default().fg(CREAM))])
            })
            .collect()
    };

    frame.render_widget(
        Paragraph::new(tools_text).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(if focused { AMBER } else { BRASS }))
                .title(Span::styled(" Tools ", Style::default().fg(title_color))),
        ),
        tools_area,
    );

    // ── Quick nav / direct counters ───────────────────────────────────────────
    let quick_lines = vec![
        Line::from(vec![
            Span::styled("Web   ", Style::default().fg(BRASS)),
            Span::styled(
                format!("{} / {}", sidebar.usage.web_search_requests, sidebar.usage.web_fetch_requests),
                Style::default().fg(CREAM),
            ),
        ]),
        Line::from(vec![
            Span::styled("Total ", Style::default().fg(BRASS)),
            Span::styled(format!("{}", sidebar.usage.total_tokens()), Style::default().fg(CREAM)),
        ]),
        Line::from(Span::styled("  direct from Claude JSONL", Style::default().fg(STONE))),
    ];
    frame.render_widget(
        Paragraph::new(quick_lines).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(if focused { AMBER } else { BRASS }))
                .title(Span::styled(" Source ", Style::default().fg(title_color))),
        ),
        quick_area,
    );
}

// ── Status bar (full width) ───────────────────────────────────────────────────

fn render_status_bar(
    frame: &mut Frame,
    area: Rect,
    session: &SessionState,
    model: &str,
    focus: CockpitFocus,
) {
    let sep = Span::styled(" │ ", Style::default().fg(STONE).bg(CHARCOAL));

    let agent_span = Span::styled(
        format!(" {} ", session.agent_name),
        Style::default().fg(AMBER).bg(CHARCOAL).add_modifier(Modifier::BOLD),
    );
    let model_span = Span::styled(model.to_string(), Style::default().fg(TAN).bg(CHARCOAL));

    let (status_label, status_fg) = agent_status_display(&session.status);
    let status_span = Span::styled(status_label, Style::default().fg(status_fg).bg(CHARCOAL));

    let tokens = session.metrics.total_tokens();
    let token_span = Span::styled(
        format!("tok: {}", tokens),
        Style::default().fg(BRASS).bg(CHARCOAL),
    );

    let focus_label = match focus {
        CockpitFocus::Sessions => "Sessions",
        CockpitFocus::Input    => "Input",
        CockpitFocus::Terminal => "Terminal",
        CockpitFocus::Sidebar  => "Sidebar",
    };
    let focus_span = Span::styled(
        format!("focus: {}", focus_label),
        Style::default().fg(STONE).bg(CHARCOAL),
    );

    let keys_span = Span::styled(
        " Tab:cycle  Ctrl+J:term  Esc:input  Ctrl+Q:quit ",
        Style::default().fg(STONE).bg(CHARCOAL),
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
        focus_span,
        sep.clone(),
        keys_span,
    ]);

    frame.render_widget(Paragraph::new(line).style(Style::default().bg(CHARCOAL)), area);
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Short one-line label for an [`AgentStatus`].
fn agent_status_label(status: &AgentStatus) -> String {
    match status {
        AgentStatus::Starting => "Starting…".to_string(),
        AgentStatus::Idle => "Idle".to_string(),
        AgentStatus::Thinking => "Thinking…".to_string(),
        AgentStatus::RunningTool { name } => format!("▶ {}", name),
        AgentStatus::WaitingApproval { tool_name } => format!("⚠ Approve: {}", tool_name),
        AgentStatus::Exited { code } => format!("Exited ({})", code.unwrap_or(-1)),
        AgentStatus::Error { message } => {
            if message.len() > 30 {
                format!("Error: {}…", &message[..29])
            } else {
                format!("Error: {}", message)
            }
        }
    }
}

/// Returns `(label, color)` for a given agent status.
fn agent_status_display(status: &AgentStatus) -> (String, Color) {
    match status {
        AgentStatus::Starting => ("Starting…".to_string(), STONE),
        AgentStatus::Idle => ("Idle".to_string(), SPROUT),
        AgentStatus::Thinking => ("Thinking…".to_string(), AMBER),
        AgentStatus::RunningTool { name } => (format!("▶ {}", name), AMBER),
        AgentStatus::WaitingApproval { tool_name } => {
            (format!("⚠ Approve: {}", tool_name), ROSE)
        }
        AgentStatus::Exited { code } => (format!("Exited ({})", code.unwrap_or(-1)), MUTED),
        AgentStatus::Error { message } => {
            let short = if message.len() > 30 {
                format!("{}…", &message[..29])
            } else {
                message.clone()
            };
            (format!("Error: {}", short), ROSE)
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::state::{AgentStatus, CockpitFocus, SessionState};

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
        assert_eq!(color, ROSE);
    }

    #[test]
    fn session_state_has_tokens_used() {
        let s = SessionState::new("s-1", "claude");
        assert_eq!(s.tokens_used, 0);
    }

    #[test]
    fn session_state_default_focus_is_input() {
        let s = SessionState::new("s-1", "claude");
        assert_eq!(s.cockpit_focus, CockpitFocus::Input);
    }

    #[test]
    fn agent_status_label_for_all_variants() {
        assert!(agent_status_label(&AgentStatus::Starting).contains("Starting"));
        assert!(agent_status_label(&AgentStatus::Idle).contains("Idle"));
        assert!(agent_status_label(&AgentStatus::Thinking).contains("Thinking"));
        assert!(
            agent_status_label(&AgentStatus::RunningTool { name: "grep".into() })
                .contains("grep")
        );
        assert!(
            agent_status_label(&AgentStatus::WaitingApproval { tool_name: "shell".into() })
                .contains("shell")
        );
        assert!(
            agent_status_label(&AgentStatus::Exited { code: Some(1) }).contains("Exited")
        );
        assert!(
            agent_status_label(&AgentStatus::Error { message: "oops".into() })
                .contains("oops")
        );
    }

    // ── CockpitFocus cycling ──────────────────────────────────────────────────

    #[test]
    fn cockpit_focus_tab_cycle() {
        assert_eq!(CockpitFocus::Sessions.next(), CockpitFocus::Input);
        assert_eq!(CockpitFocus::Input.next(),    CockpitFocus::Terminal);
        assert_eq!(CockpitFocus::Terminal.next(), CockpitFocus::Sidebar);
        assert_eq!(CockpitFocus::Sidebar.next(),  CockpitFocus::Sessions);
    }

    #[test]
    fn cockpit_focus_shift_tab_cycle() {
        assert_eq!(CockpitFocus::Sessions.prev(), CockpitFocus::Sidebar);
        assert_eq!(CockpitFocus::Input.prev(),    CockpitFocus::Sessions);
        assert_eq!(CockpitFocus::Terminal.prev(), CockpitFocus::Input);
        assert_eq!(CockpitFocus::Sidebar.prev(),  CockpitFocus::Terminal);
    }

    #[test]
    fn cockpit_focus_full_tab_round_trip() {
        let mut f = CockpitFocus::Input;
        for _ in 0..4 {
            f = f.next();
        }
        assert_eq!(f, CockpitFocus::Input, "4 Tabs should wrap back to Input");
    }

    #[test]
    fn cockpit_focus_full_shift_tab_round_trip() {
        let mut f = CockpitFocus::Input;
        for _ in 0..4 {
            f = f.prev();
        }
        assert_eq!(f, CockpitFocus::Input, "4 Shift+Tabs should wrap back to Input");
    }
}
