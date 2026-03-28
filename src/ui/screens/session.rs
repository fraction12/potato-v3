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
//! `Ctrl+Q` leaves terminal focus back to Input.
//! `Esc` passes through to agent PTY when terminal is focused.
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
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, List, ListItem, ListState, Paragraph, Widget},
};

use crate::app::state::{AgentStatus, AppScreen, AppState, CockpitFocus, Overlay, SessionState};
use crate::claude_log::{ClaudeSidebarData, ClaudeToolStatus};
use crate::session::store::unix_now;
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

    // ── Center column: [pty_panes (min)] | [input_bar 3 lines] ───────────────
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

    // ── Render PTY panes (side-by-side if multiple) ──────────────────────────
    let n_panes = state.panes.len();
    let active_pane_idx = state.panes.active_index();

    if n_panes == 0 {
        // Fall back to legacy single PTY or placeholder.
        render_pty_viewport_legacy(frame, pty_area, state, focus);
    } else if n_panes == 1 {
        render_pane_viewport(frame, pty_area, state, 0, active_pane_idx == 0, focus);
    } else {
        // Split the PTY area horizontally for each pane.
        let constraints: Vec<Constraint> = (0..n_panes)
            .map(|_| Constraint::Ratio(1, n_panes as u32))
            .collect();
        let pane_areas = Layout::horizontal(constraints).split(pty_area);
        for i in 0..n_panes {
            render_pane_viewport(frame, pane_areas[i], state, i, i == active_pane_idx, focus);
        }
    }

    // Now borrow session immutably for the rest.
    let AppScreen::Session(ref session) = state.screen else { return };

    render_left_rail(frame, left_area, state, focus);
    render_input_bar(frame, input_area, session, focus);
    render_right_rail(frame, right_area, state, focus);
    render_status_bar(frame, status_area, session, &state.model, focus, state.panes.len());

    // ── Autocomplete popup (above input bar, when in command mode) ────────────
    if focus == CockpitFocus::Input && session.input_buffer.starts_with('/') {
        let prefix = &session.input_buffer[1..];
        let matches = crate::commands::registry::completions(prefix);
        if !matches.is_empty() {
            render_command_autocomplete(frame, input_area, area, &matches, session.command_selected);
        }
    }

    // ── Overlay (rendered on top of everything) ───────────────────────────────
    if let Some(ref overlay) = session.overlay {
        match overlay {
            Overlay::Help => {
                let help = crate::ui::overlays::help::HelpOverlay::new();
                crate::ui::overlays::Overlay::render(&help, frame, area);
            }
            Overlay::Sessions => {
                // Sessions overlay stub — show a simple "Coming soon" message.
                render_sessions_overlay_stub(frame, area);
            }
            Overlay::AgentPicker => {
                let rows = crate::ui::overlays::agent_picker::build_agent_rows();
                crate::ui::overlays::agent_picker::render_agent_picker(
                    frame,
                    area,
                    &session.agent_picker,
                    &rows,
                );
            }
        }
    }
}

// ── Left rail — agents + sessions ─────────────────────────────────────────────

fn render_left_rail(frame: &mut Frame, area: Rect, state: &AppState, focus: CockpitFocus) {
    // Detect agents for the rail display.
    let agent_rows = crate::ui::overlays::agent_picker::build_agent_rows();
    // Agents section: border (2) + N item rows, capped at 5.
    let agent_row_count = agent_rows.len().min(3) as u16;
    let agents_height = 2 + agent_row_count; // border top + bottom + rows
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(agents_height),
            Constraint::Min(4),
        ])
        .split(area);
    let agents_area = chunks[0];
    let sessions_area = chunks[1];

    render_agents_section(frame, agents_area, state, focus, &agent_rows);
    render_sessions_section(frame, sessions_area, state, focus);
}

/// Top part of the left rail — agent list with availability indicators.
fn render_agents_section(
    frame: &mut Frame,
    area: Rect,
    state: &AppState,
    focus: CockpitFocus,
    agent_rows: &[crate::ui::overlays::agent_picker::AgentRow],
) {
    let focused = focus == CockpitFocus::Agents;
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

    let selected_idx = state
        .session()
        .map(|s| s.selected_agent)
        .unwrap_or(0);

    let items: Vec<ListItem<'_>> = agent_rows
        .iter()
        .enumerate()
        .map(|(idx, row)| {
            let is_selected = idx == selected_idx && focused;
            // Status indicator: ● for available, ○ for unavailable
            let indicator = if row.available { "●" } else { "○" };
            let indicator_color = if row.available {
                Color::Rgb(100, 200, 100)
            } else {
                Color::Rgb(120, 80, 80)
            };
            let name_color = if row.available {
                if is_selected { CREAM } else { TAN }
            } else {
                STONE
            };
            let bg_color = if is_selected {
                Color::Rgb(45, 30, 20)
            } else {
                BG
            };

            // Truncate name to fit the narrow rail.
            let max_name = area.width.saturating_sub(5) as usize;
            let display_name = if row.display_name.len() > max_name {
                &row.display_name[..max_name]
            } else {
                &row.display_name
            };

            let line = Line::from(vec![
                Span::styled(
                    format!(" {indicator} "),
                    Style::default().fg(indicator_color),
                ),
                Span::styled(
                    display_name.to_string(),
                    Style::default()
                        .fg(name_color)
                        .bg(bg_color)
                        .add_modifier(if is_selected { Modifier::BOLD } else { Modifier::empty() }),
                ),
            ]);
            ListItem::new(line)
        })
        .collect();

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(border_style)
        .title(Span::styled(" Agents ", title_style));

    let list = List::new(items)
        .block(block)
        .style(Style::default().fg(STONE).bg(BG));

    frame.render_widget(list, area);
}

/// Bottom part of the left rail — historical session list.
fn render_sessions_section(frame: &mut Frame, area: Rect, state: &AppState, focus: CockpitFocus) {
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

    let active_session_id = state
        .session()
        .and_then(|s| s.claude_session_id.as_deref())
        .map(str::to_string);

    let selected_idx = state
        .session()
        .map(|s| s.selected_session)
        .unwrap_or(0);

    let rail = &state.rail_sessions;

    // Inner width for text wrapping (border + 1 padding each side).
    let inner_w = area.width.saturating_sub(4) as usize;

    let items: Vec<ListItem<'static>> = if rail.is_empty() {
        vec![ListItem::new(Line::from(Span::styled(
            " No sessions yet",
            Style::default().fg(STONE),
        )))]
    } else {
        rail.iter()
            .enumerate()
            .map(|(idx, s)| {
                let is_active = active_session_id.as_deref() == Some(s.id.as_str());
                let is_selected = idx == selected_idx;

                let marker = if is_active { "● " } else { "  " };
                let marker_style = if is_active {
                    Style::default().fg(SPROUT)
                } else {
                    Style::default().fg(STONE)
                };

                // Title: use stored title, fall back to short id.
                let title_raw = if s.title.is_empty() {
                    s.id.chars().take(10).collect::<String>()
                } else {
                    s.title.clone()
                };

                let item_title_style = if is_selected {
                    Style::default()
                        .fg(CREAM)
                        .add_modifier(Modifier::BOLD)
                        .bg(Color::Rgb(45, 30, 20))
                } else if is_active {
                    Style::default().fg(CREAM)
                } else {
                    Style::default().fg(TAN)
                };

                // Wrap title across lines if wider than the rail.
                let title_w = inner_w.saturating_sub(2); // account for marker
                let title_lines = wrap_text(&title_raw, title_w);

                let row_bg = if is_selected && focused {
                    Color::Rgb(45, 30, 20)
                } else {
                    BG
                };

                let mut lines: Vec<Line<'static>> = Vec::new();

                // First title line gets the marker prefix.
                if let Some(first) = title_lines.first() {
                    lines.push(Line::from(vec![
                        Span::styled(marker.to_string(), marker_style),
                        Span::styled(first.clone(), item_title_style),
                    ]));
                }
                // Continuation lines indented to match.
                for cont in title_lines.iter().skip(1) {
                    lines.push(Line::from(vec![
                        Span::styled("  ", marker_style),
                        Span::styled(cont.clone(), item_title_style),
                    ]));
                }

                // Relative date + token count.
                let rel_date = relative_date(s.updated_at);
                let tok = fmt_tokens_small(s.total_tokens());
                let meta = format!("{} {}", rel_date, tok);
                let meta_w = inner_w.saturating_sub(2);
                let meta_truncated: String = meta.chars().take(meta_w).collect();

                lines.push(Line::from(Span::styled(
                    format!("  {}", meta_truncated),
                    Style::default().fg(STONE),
                )));

                ListItem::new(lines).style(Style::default().bg(row_bg))
            })
            .collect()
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(border_style)
        .title(Span::styled(" Sessions ", title_style));

    let list = List::new(items)
        .block(block)
        .highlight_style(Style::default()) // selection tracked manually via row_bg
        .style(Style::default().fg(STONE).bg(BG));

    // Use ListState so ratatui handles scroll offset automatically.
    let mut list_state = ListState::default();
    if !rail.is_empty() {
        list_state.select(Some(selected_idx.min(rail.len().saturating_sub(1))));
    }

    frame.render_stateful_widget(list, area, &mut list_state);
}

// ── Center — PTY viewport ─────────────────────────────────────────────────────

/// Render a single pane's PTY viewport from the PaneManager.
fn render_pane_viewport(
    frame: &mut Frame,
    area: Rect,
    state: &mut AppState,
    pane_idx: usize,
    is_active: bool,
    focus: CockpitFocus,
) {
    let focused = focus == CockpitFocus::Terminal && is_active;
    let border_style = if focused {
        Style::default().fg(AMBER)
    } else if is_active {
        Style::default().fg(BRASS)
    } else {
        Style::default().fg(STONE)
    };
    let title_style = if focused {
        Style::default().fg(AMBER).add_modifier(Modifier::BOLD)
    } else if is_active {
        Style::default().fg(TAN).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(STONE)
    };

    let inner_cols = area.width.saturating_sub(2);
    let inner_rows = area.height.saturating_sub(2);

    let pane = state.panes.get(pane_idx);

    if let Some(pane) = pane {
        let desired_scroll = pane.session.terminal_scroll;

        if let Some(ref pty) = pane.pty {
            let _ = pty.resize(inner_cols.max(1), inner_rows.max(1));
            let actual_scroll = pty.set_scrollback(desired_scroll);

            let active_marker = if is_active { " ●" } else { "" };
            let pane_label = format!(" Claude {}{} ", pane_idx + 1, active_marker);
            let title = if actual_scroll > 0 {
                Span::styled(format!("{} ↑{} ", pane_label.trim(), actual_scroll), title_style)
            } else {
                Span::styled(pane_label, title_style)
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

            // Sync scroll back to pane state.
            if let Some(pane_mut) = state.panes.get_mut(pane_idx) {
                pane_mut.session.terminal_scroll = actual_scroll;
            }
        } else {
            // Pane exists but no PTY — starting up.
            let placeholder = Paragraph::new("  Starting…")
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_style(border_style)
                        .title(Span::styled(format!(" Claude {} ", pane_idx + 1), title_style)),
                )
                .style(Style::default().fg(STONE));
            frame.render_widget(placeholder, area);
        }
    }

    // Focus indicator.
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

/// Placeholder shown when no panes are active (zero PTYs).
fn render_pty_viewport_legacy(frame: &mut Frame, area: Rect, _state: &mut AppState, _focus: CockpitFocus) {
    let placeholder = Paragraph::new(
        "\n  No active session.\n  Select an agent above and press Enter.",
    )
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(BRASS))
            .title(Span::styled(" Terminal ", Style::default().fg(STONE))),
    )
    .style(Style::default().fg(STONE));
    frame.render_widget(placeholder, area);
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

// ── Command autocomplete popup ────────────────────────────────────────────────

/// Maximum number of commands shown in the autocomplete popup at once.
const AUTOCOMPLETE_MAX_ROWS: usize = 6;

/// Render the slash-command autocomplete popup above the input bar.
///
/// `input_area` is the input bar rect (used for positioning).
/// `screen_area` is the full terminal area (used for bounds clamping).
/// `matches` is the filtered list of commands; `selected` is the highlighted index.
fn render_command_autocomplete(
    frame: &mut Frame,
    input_area: Rect,
    screen_area: Rect,
    matches: &[&crate::commands::registry::SlashCommand],
    selected: usize,
) {
    if matches.is_empty() {
        return;
    }

    let row_count = matches.len().min(AUTOCOMPLETE_MAX_ROWS) as u16;
    let popup_height = row_count + 2; // 2 for borders
    let popup_width = 54_u16.min(screen_area.width);

    // Position directly above the input bar, left-aligned with input.
    let popup_y = input_area.y.saturating_sub(popup_height);
    let popup_x = input_area.x;

    let popup_area = Rect::new(popup_x, popup_y, popup_width, popup_height)
        .intersection(screen_area);

    // Clear background behind the popup.
    frame.render_widget(Clear, popup_area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(AMBER))
        .style(Style::default().bg(CHARCOAL));

    let inner = block.inner(popup_area);
    frame.render_widget(block, popup_area);

    for (i, cmd) in matches.iter().enumerate().take(AUTOCOMPLETE_MAX_ROWS) {
        if i as u16 >= inner.height {
            break;
        }
        let row_area = Rect::new(inner.x, inner.y + i as u16, inner.width, 1);
        let is_selected = i == selected;

        let bg = if is_selected { AMBER } else { CHARCOAL };
        let fg = if is_selected { BG } else { STONE };
        let desc_fg = if is_selected { BG } else { Color::Rgb(90, 90, 90) };

        let name_text = format!("/{:<12}", cmd.name);
        let desc_text = format!(" {}", cmd.description);

        let spans = vec![
            Span::styled(
                name_text,
                Style::default().fg(fg).bg(bg).add_modifier(if is_selected { Modifier::BOLD } else { Modifier::empty() }),
            ),
            Span::styled(desc_text, Style::default().fg(desc_fg).bg(bg)),
        ];

        Paragraph::new(Line::from(spans)).render(row_area, frame.buffer_mut());
    }
}

// ── Sessions overlay stub ─────────────────────────────────────────────────────

/// Placeholder sessions overlay (full picker is a future phase).
fn render_sessions_overlay_stub(frame: &mut Frame, area: Rect) {
    let width = 40_u16.min(area.width);
    let height = 5_u16.min(area.height);
    let x = area.left() + area.width.saturating_sub(width) / 2;
    let y = area.top() + area.height.saturating_sub(height) / 2;
    let popup_area = Rect::new(x, y, width, height).intersection(area);

    frame.render_widget(Clear, popup_area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(AMBER))
        .title(" Sessions ")
        .style(Style::default().bg(CHARCOAL));

    let inner = block.inner(popup_area);
    frame.render_widget(block, popup_area);

    let msg = Paragraph::new("Session picker coming soon.\nPress Esc to close.")
        .style(Style::default().fg(STONE).bg(CHARCOAL));
    frame.render_widget(msg, inner);
}

// ── Right rail — metrics / tools / sidebar ────────────────────────────────────

fn render_right_rail(frame: &mut Frame, area: Rect, state: &AppState, focus: CockpitFocus) {
    let focused = focus == CockpitFocus::Sidebar;
    let title_color = if focused { AMBER } else { TAN };
    let border_fg = if focused { AMBER } else { BRASS };
    let label = Style::default().fg(STONE);
    let value = Style::default().fg(CREAM);

    // Inner width for truncation (border + 1 char padding each side).
    let inner_w = area.width.saturating_sub(4) as usize;

    // Split sidebar vertically: Metrics | Tools | Totals
    let [metrics_area, tools_area, totals_area] = Layout::vertical([
        Constraint::Length(9),
        Constraint::Min(5),
        Constraint::Length(6),
    ])
    .areas(area);

    // Read metrics from the active pane's log tracker (or fall back to legacy).
    let sidebar = state
        .panes
        .active_pane()
        .and_then(|p| p.log.as_ref())
        .map(|t| t.snapshot())
        .unwrap_or_default();

    // ── Metrics ───────────────────────────────────────────────────────────────

    let model_short = sidebar
        .model
        .as_deref()
        .unwrap_or("—")
        .strip_prefix("claude-")
        .unwrap_or(sidebar.model.as_deref().unwrap_or("—"));

    let metrics_text = vec![
        Line::from(Span::raw("")),
        metric_line(" Model", model_short, label, value),
        metric_line(" Turns", &sidebar.turns.to_string(), label, value),
        metric_line(" In",    &fmt_tokens(sidebar.usage.input_tokens), label, value),
        metric_line(" Out",   &fmt_tokens(sidebar.usage.output_tokens), label, value),
        metric_line(" Cache", &fmt_tokens(sidebar.usage.cache_read_input_tokens), label, value),
        metric_line(" Stop",  sidebar.last_stop_reason.as_deref().unwrap_or("—"), label, value),
    ];

    frame.render_widget(
        Paragraph::new(metrics_text)
            .block(sidebar_block(" Claude ", title_color, border_fg))
            .style(Style::default().bg(BG)),
        metrics_area,
    );

    // ── Tools ─────────────────────────────────────────────────────────────────

    let max_tools = tools_area.height.saturating_sub(2) as usize; // minus borders

    let tools_text: Vec<Line> = if sidebar.tools.is_empty() {
        vec![
            Line::from(Span::raw("")),
            Line::from(Span::styled(" waiting…", Style::default().fg(STONE))),
        ]
    } else {
        sidebar
            .tools
            .iter()
            .rev()
            .take(max_tools)
            .map(|e| {
                let icon = match e.status {
                    ClaudeToolStatus::Done    => Span::styled(" ✓ ", Style::default().fg(SPROUT)),
                    ClaudeToolStatus::Error   => Span::styled(" ✗ ", Style::default().fg(ROSE)),
                    ClaudeToolStatus::Running => Span::styled(" ⏳", Style::default().fg(AMBER)),
                };
                let max_name = inner_w.saturating_sub(3);
                let name = truncate_str(&e.name, max_name);
                Line::from(vec![icon, Span::styled(name, value)])
            })
            .collect()
    };

    frame.render_widget(
        Paragraph::new(tools_text)
            .block(sidebar_block(" Tools ", title_color, border_fg))
            .style(Style::default().bg(BG)),
        tools_area,
    );

    // ── Totals ────────────────────────────────────────────────────────────────

    let totals_text = vec![
        Line::from(Span::raw("")),
        metric_line(" Total", &fmt_tokens(sidebar.usage.total_tokens()), label, value),
        metric_line(" Web",   &format!("{}s {}f", sidebar.usage.web_search_requests, sidebar.usage.web_fetch_requests), label, value),
        metric_line(" New$",  &fmt_tokens(sidebar.usage.cache_creation_input_tokens), label, value),
    ];

    frame.render_widget(
        Paragraph::new(totals_text)
            .block(sidebar_block(" Totals ", title_color, border_fg))
            .style(Style::default().bg(BG)),
        totals_area,
    );
}

/// Render a sidebar section block with consistent styling.
fn sidebar_block(title: &str, title_color: Color, border_color: Color) -> Block<'_> {
    Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color))
        .title(Span::styled(title, Style::default().fg(title_color)))
}

/// Render a `label  value` metric line with right-aligned value appearance.
fn metric_line<'a>(
    name: &'a str,
    val: &str,
    label_style: Style,
    value_style: Style,
) -> Line<'a> {
    // Pad label to 6 chars for alignment.
    let padded_label = format!("{:<6}", name);
    Line::from(vec![
        Span::styled(padded_label, label_style),
        Span::styled(format!(" {}", val), value_style),
    ])
}

/// Format token counts with `k` suffix for readability.
fn fmt_tokens(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 10_000 {
        format!("{:.1}k", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}

/// Truncate a string to `max` chars, appending `…` if needed.
fn truncate_str(s: &str, max: usize) -> String {
    if s.len() <= max || max < 2 {
        s.to_string()
    } else {
        format!("{}…", &s[..max - 1])
    }
}

// ── Status bar (full width) ───────────────────────────────────────────────────

fn render_status_bar(
    frame: &mut Frame,
    area: Rect,
    session: &SessionState,
    model: &str,
    focus: CockpitFocus,
    pane_count: usize,
) {
    let sep = Span::styled(" │ ", Style::default().fg(STONE).bg(CHARCOAL));

    let agent_span = Span::styled(
        format!(" {} ", session.agent_name),
        Style::default().fg(AMBER).bg(CHARCOAL).add_modifier(Modifier::BOLD),
    );
    let model_span = Span::styled(model.to_string(), Style::default().fg(TAN).bg(CHARCOAL));

    let (status_label, status_fg) = agent_status_display(&session.status);
    let status_span = Span::styled(status_label, Style::default().fg(status_fg).bg(CHARCOAL));

    let token_span = Span::styled(
        format!("tok: {}", fmt_tokens(session.tokens_used)),
        Style::default().fg(BRASS).bg(CHARCOAL),
    );

    let focus_label = match focus {
        CockpitFocus::Agents   => "Agents",
        CockpitFocus::Sessions => "Sessions",
        CockpitFocus::Input    => "Input",
        CockpitFocus::Terminal => "Terminal",
        CockpitFocus::Sidebar  => "Sidebar",
    };
    let focus_span = Span::styled(
        format!("focus: {}", focus_label),
        Style::default().fg(STONE).bg(CHARCOAL),
    );

    let keys_text = if pane_count > 1 {
        " Alt+[/]:switch pane  Ctrl+J:term  Ctrl+W:close pane  Ctrl+\\:quit "
    } else {
        " Tab:cycle  Ctrl+J:term  Ctrl+Q:exit term  Ctrl+W:close  ?:help "
    };
    let keys_span = Span::styled(
        keys_text,
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

/// Word-wrap `text` into lines of at most `width` characters.
///
/// Breaks on whitespace when possible; hard-breaks long words.
fn wrap_text(text: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![text.to_string()];
    }
    let mut lines = Vec::new();
    let mut current = String::new();
    let mut current_len: usize = 0;

    for word in text.split_whitespace() {
        let wlen = word.chars().count();

        // Hard-break words longer than the available width.
        if wlen > width {
            // Flush current line first.
            if current_len > 0 {
                lines.push(std::mem::take(&mut current));
                current_len = 0;
            }
            let mut chars = word.chars().peekable();
            while chars.peek().is_some() {
                let chunk: String = chars.by_ref().take(width).collect();
                lines.push(chunk);
            }
            continue;
        }

        if current_len == 0 {
            current.push_str(word);
            current_len = wlen;
        } else if current_len + 1 + wlen <= width {
            current.push(' ');
            current.push_str(word);
            current_len += 1 + wlen;
        } else {
            lines.push(std::mem::take(&mut current));
            current = word.to_string();
            current_len = wlen;
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

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

// ── Left-rail helpers ─────────────────────────────────────────────────────────

/// Format token count compactly for the narrow left rail.
fn fmt_tokens_small(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.0}k", n as f64 / 1_000.0)
    } else if n == 0 {
        String::new()
    } else {
        n.to_string()
    }
}

/// Convert a Unix timestamp to a human-readable relative string.
fn relative_date(ts: i64) -> String {
    let now = unix_now();
    let secs = now.saturating_sub(ts);
    if secs < 60 {
        "just now".to_string()
    } else if secs < 3600 {
        format!("{}m ago", secs / 60)
    } else if secs < 86400 {
        format!("{}h ago", secs / 3600)
    } else if secs < 86400 * 2 {
        "yesterday".to_string()
    } else {
        // Format as "Mar 24"-style using chrono.
        use chrono::{TimeZone, Utc};
        let dt = Utc.timestamp_opt(ts, 0).single().unwrap_or_else(Utc::now);
        dt.format("%b %-d").to_string()
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
        assert_eq!(CockpitFocus::Agents.next(),   CockpitFocus::Sessions);
        assert_eq!(CockpitFocus::Sessions.next(), CockpitFocus::Input);
        assert_eq!(CockpitFocus::Input.next(),    CockpitFocus::Terminal);
        assert_eq!(CockpitFocus::Terminal.next(), CockpitFocus::Sidebar);
        assert_eq!(CockpitFocus::Sidebar.next(),  CockpitFocus::Agents);
    }

    #[test]
    fn cockpit_focus_shift_tab_cycle() {
        assert_eq!(CockpitFocus::Agents.prev(),   CockpitFocus::Sidebar);
        assert_eq!(CockpitFocus::Sessions.prev(), CockpitFocus::Agents);
        assert_eq!(CockpitFocus::Input.prev(),    CockpitFocus::Sessions);
        assert_eq!(CockpitFocus::Terminal.prev(), CockpitFocus::Input);
        assert_eq!(CockpitFocus::Sidebar.prev(),  CockpitFocus::Terminal);
    }

    #[test]
    fn cockpit_focus_full_tab_round_trip() {
        let mut f = CockpitFocus::Input;
        for _ in 0..5 {
            f = f.next();
        }
        assert_eq!(f, CockpitFocus::Input, "5 Tabs should wrap back to Input");
    }

    #[test]
    fn cockpit_focus_full_shift_tab_round_trip() {
        let mut f = CockpitFocus::Input;
        for _ in 0..5 {
            f = f.prev();
        }
        assert_eq!(f, CockpitFocus::Input, "5 Shift+Tabs should wrap back to Input");
    }

    // ── Left-rail helpers ─────────────────────────────────────────────────────

    #[test]
    fn fmt_tokens_small_zero() {
        assert_eq!(fmt_tokens_small(0), "");
    }

    #[test]
    fn fmt_tokens_small_small() {
        assert_eq!(fmt_tokens_small(500), "500");
    }

    #[test]
    fn fmt_tokens_small_kilo() {
        assert_eq!(fmt_tokens_small(12_500), "12k");
    }

    #[test]
    fn fmt_tokens_small_mega() {
        assert!(fmt_tokens_small(1_500_000).contains('M'));
    }

    #[test]
    fn relative_date_just_now() {
        let now = unix_now();
        assert_eq!(relative_date(now - 5), "just now");
    }

    #[test]
    fn relative_date_minutes_ago() {
        let now = unix_now();
        let label = relative_date(now - 300); // 5 minutes
        assert!(label.contains('m') && label.contains("ago"), "got: {label}");
    }

    #[test]
    fn relative_date_hours_ago() {
        let now = unix_now();
        let label = relative_date(now - 7200); // 2h
        assert!(label.contains('h') && label.contains("ago"), "got: {label}");
    }

    #[test]
    fn relative_date_yesterday() {
        let now = unix_now();
        assert_eq!(relative_date(now - 86_500), "yesterday");
    }

    // ── wrap_text ─────────────────────────────────────────────────────────

    #[test]
    fn wrap_text_short() {
        assert_eq!(wrap_text("hello", 20), vec!["hello"]);
    }

    #[test]
    fn wrap_text_wraps() {
        assert_eq!(
            wrap_text("hello world foo", 11),
            vec!["hello world", "foo"],
        );
    }

    #[test]
    fn wrap_text_hard_break() {
        let long = "abcdefghij";
        let lines = wrap_text(long, 4);
        assert_eq!(lines, vec!["abcd", "efgh", "ij"]);
    }

    #[test]
    fn wrap_text_empty() {
        assert_eq!(wrap_text("", 10), vec![""]);
    }
}
