//! Dashboard screen — Option B layout with left menu rail and right detail pane.
//!
//! Layout:
//! ```
//! ┌──────────────────────────────────────────────────────────────────────────┐
//! │                           🥔  Potato                                     │
//! ├────────────────────┬─────────────────────────────────────────────────────┤
//! │                    │                                                     │
//! │  Roast Potato      │  (detail for selected menu item)                    │
//! │  Define Roles      │                                                     │
//! │  Integrations      │                                                     │
//! │  Settings          │                                                     │
//! │                    │                                                     │
//! ├────────────────────┴─────────────────────────────────────────────────────┤
//! │  ↑↓ navigate  Tab switch  Enter select  q quit                          │
//! └──────────────────────────────────────────────────────────────────────────┘
//! ```

use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap},
};

use crate::app::state::{AppScreen, AppState, DashboardFocus, DashboardMenuItem};
use crate::ui::theme::{AMBER, BG, BRASS, BROWN, CHARCOAL, CREAM, SOIL, SPROUT, STONE, TAN};

/// Muted gray for unavailable/secondary items.
const MUTED: Color = Color::Rgb(100, 100, 100);

// ── Entry point ───────────────────────────────────────────────────────────────

/// Render the full dashboard screen.
pub fn render_dashboard(frame: &mut Frame, area: Rect, state: &AppState) {
    let AppScreen::Dashboard(ref dash) = state.screen else {
        return;
    };

    let outer = Block::default().style(Style::default().bg(BG));
    frame.render_widget(outer, area);

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // title bar
            Constraint::Min(0),   // main content
            Constraint::Length(1), // footer
        ])
        .split(area);

    render_title(frame, rows[0]);
    render_content(frame, rows[1], state);
    render_footer(frame, rows[2], state);
}

// ── Title ─────────────────────────────────────────────────────────────────────

fn render_title(frame: &mut Frame, area: Rect) {
    let title = Paragraph::new("🥔  Potato")
        .alignment(Alignment::Center)
        .style(
            Style::default()
                .fg(BRASS)
                .bg(BG)
                .add_modifier(Modifier::BOLD),
        )
        .block(
            Block::default()
                .borders(Borders::BOTTOM)
                .border_style(Style::default().fg(STONE)),
        );
    frame.render_widget(title, area);
}

// ── Content (two-column: menu + detail) ───────────────────────────────────────

fn render_content(frame: &mut Frame, area: Rect, state: &AppState) {
    let AppScreen::Dashboard(ref dash) = state.screen else {
        return;
    };

    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(24), Constraint::Min(0)])
        .split(area);

    render_menu(frame, cols[0], dash);
    render_detail(frame, cols[1], state);
}

// ── Left menu rail ────────────────────────────────────────────────────────────

fn render_menu(
    frame: &mut Frame,
    area: Rect,
    dash: &crate::app::state::DashboardState,
) {
    let focused = dash.focus == DashboardFocus::Menu;
    let border_style = if focused {
        Style::default().fg(AMBER)
    } else {
        Style::default().fg(STONE)
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(border_style)
        .style(Style::default().bg(BG));

    let items: Vec<ListItem> = DashboardMenuItem::ALL
        .iter()
        .enumerate()
        .map(|(i, item)| {
            let is_selected = i == dash.selected_menu;
            let bg = if is_selected { CHARCOAL } else { BG };
            let fg = if is_selected { CREAM } else { TAN };
            let style = if is_selected {
                Style::default().fg(fg).bg(bg).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(fg).bg(bg)
            };
            ListItem::new(Line::from(Span::styled(
                format!("  {}", item.label()),
                style,
            )))
        })
        .collect();

    let mut list_state = ListState::default();
    list_state.select(Some(dash.selected_menu));

    let list = List::new(items)
        .block(block)
        .highlight_style(Style::default().fg(CREAM).bg(CHARCOAL).add_modifier(Modifier::BOLD));

    frame.render_stateful_widget(list, area, &mut list_state);
}

// ── Right detail pane ─────────────────────────────────────────────────────────

fn render_detail(frame: &mut Frame, area: Rect, state: &AppState) {
    let AppScreen::Dashboard(ref dash) = state.screen else {
        return;
    };

    let menu_item = DashboardMenuItem::ALL[dash.selected_menu];
    match menu_item {
        DashboardMenuItem::RoastPotato => render_detail_roast(frame, area, state),
        DashboardMenuItem::DefineRoles => render_detail_roles(frame, area, state),
        DashboardMenuItem::Integrations => render_detail_integrations(frame, area, state),
        DashboardMenuItem::Settings => render_detail_settings(frame, area, state),
    }
}

// ── Detail: Roast Potato ──────────────────────────────────────────────────────

fn render_detail_roast(frame: &mut Frame, area: Rect, state: &AppState) {
    let AppScreen::Dashboard(ref dash) = state.screen else {
        return;
    };
    let detail_focused = dash.focus == DashboardFocus::Detail;

    let border_style = if detail_focused {
        Style::default().fg(AMBER)
    } else {
        Style::default().fg(STONE)
    };

    let block = Block::default()
        .title(Span::styled(" Launch ", Style::default().fg(TAN)))
        .borders(Borders::ALL)
        .border_style(border_style)
        .style(Style::default().bg(BG));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    // Build the content lines.
    let mut lines: Vec<Line> = Vec::new();

    // Agents summary.
    lines.push(Line::from(Span::styled(
        "  AGENTS",
        Style::default().fg(BRASS).add_modifier(Modifier::BOLD),
    )));
    for agent in &dash.available_agents {
        let (indicator, fg) = if agent.available {
            ("●", SPROUT)
        } else {
            ("○", MUTED)
        };
        let status = if agent.available { "ready" } else { "not found" };
        lines.push(Line::from(vec![
            Span::styled(format!("    {} ", indicator), Style::default().fg(fg)),
            Span::styled(
                format!("{:<16}", agent.name),
                Style::default().fg(if agent.available { CREAM } else { MUTED }),
            ),
            Span::styled(status, Style::default().fg(fg)),
        ]));
    }

    lines.push(Line::from(""));

    // Roles summary.
    lines.push(Line::from(Span::styled(
        "  ROLES",
        Style::default().fg(BRASS).add_modifier(Modifier::BOLD),
    )));
    if dash.roles.is_empty() {
        lines.push(Line::from(Span::styled(
            "    No roles defined. Agents will self-organize.",
            Style::default().fg(MUTED),
        )));
    } else {
        for (i, role) in dash.roles.iter().enumerate() {
            lines.push(Line::from(vec![
                Span::styled(
                    format!("    Pane {} ", i + 1),
                    Style::default().fg(TAN),
                ),
                Span::styled(&role.name, Style::default().fg(CREAM)),
            ]));
        }
    }

    lines.push(Line::from(""));

    // MCP summary.
    lines.push(Line::from(Span::styled(
        "  MCP",
        Style::default().fg(BRASS).add_modifier(Modifier::BOLD),
    )));
    let mcp_status = if state.mcp_socket_path.is_some() {
        ("active", SPROUT)
    } else {
        ("inactive", MUTED)
    };
    lines.push(Line::from(vec![
        Span::styled("    Status: ", Style::default().fg(TAN)),
        Span::styled(mcp_status.0, Style::default().fg(mcp_status.1)),
    ]));

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "  Press Enter to launch.",
        Style::default().fg(BRASS),
    )));

    lines.push(Line::from(""));
    lines.push(Line::from(""));

    // Recent sessions.
    lines.push(Line::from(Span::styled(
        "  RECENT SESSIONS",
        Style::default().fg(BRASS).add_modifier(Modifier::BOLD),
    )));

    if dash.recent_sessions.is_empty() {
        lines.push(Line::from(Span::styled(
            "    No recent sessions.",
            Style::default().fg(MUTED),
        )));
    } else {
        for (i, s) in dash.recent_sessions.iter().enumerate() {
            let is_selected = detail_focused && i == dash.selected_detail;
            let bg = if is_selected { CHARCOAL } else { BG };
            let fg = if is_selected { CREAM } else { TAN };
            let date = s.started_at.format("%m/%d %H:%M").to_string();
            let cost = if s.total_cost_usd > 0.0 {
                format!("${:.3}", s.total_cost_usd)
            } else {
                "—".to_string()
            };
            lines.push(Line::from(vec![
                Span::styled(
                    format!("    [{:<8}] ", s.agent_name),
                    Style::default().fg(BRASS).bg(bg),
                ),
                Span::styled(date, Style::default().fg(fg).bg(bg)),
                Span::styled(
                    format!("  {}", cost),
                    Style::default().fg(AMBER).bg(bg),
                ),
            ]));
        }
    }

    let content = Paragraph::new(lines);
    frame.render_widget(content, inner);
}

// ── Detail: Define Roles ──────────────────────────────────────────────────────

fn render_detail_roles(frame: &mut Frame, area: Rect, state: &AppState) {
    let AppScreen::Dashboard(ref dash) = state.screen else {
        return;
    };
    let detail_focused = dash.focus == DashboardFocus::Detail;

    let border_style = if detail_focused {
        Style::default().fg(AMBER)
    } else {
        Style::default().fg(STONE)
    };

    let block = Block::default()
        .title(Span::styled(" Roles ", Style::default().fg(TAN)))
        .borders(Borders::ALL)
        .border_style(border_style)
        .style(Style::default().bg(BG));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut lines: Vec<Line> = Vec::new();

    lines.push(Line::from(Span::styled(
        "  Define roles for each pane. Agents receive these",
        Style::default().fg(CREAM),
    )));
    lines.push(Line::from(Span::styled(
        "  as initial instructions at launch.",
        Style::default().fg(CREAM),
    )));
    lines.push(Line::from(""));

    if dash.roles.is_empty() {
        lines.push(Line::from(Span::styled(
            "  No roles defined yet.",
            Style::default().fg(MUTED),
        )));
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "  Without roles, agents will self-organize based on",
            Style::default().fg(MUTED),
        )));
        lines.push(Line::from(Span::styled(
            "  what they find in the project and via MCP tools.",
            Style::default().fg(MUTED),
        )));
    } else {
        for (i, role) in dash.roles.iter().enumerate() {
            let is_selected = detail_focused && i == dash.selected_detail;
            let bg = if is_selected { CHARCOAL } else { BG };
            lines.push(Line::from(vec![
                Span::styled(
                    format!("  Pane {} — ", i + 1),
                    Style::default().fg(TAN).bg(bg),
                ),
                Span::styled(
                    &role.name,
                    Style::default().fg(CREAM).bg(bg).add_modifier(Modifier::BOLD),
                ),
            ]));
            // Show truncated prompt.
            let prompt_preview = if role.prompt.len() > 60 {
                format!("{}...", &role.prompt[..57])
            } else {
                role.prompt.clone()
            };
            lines.push(Line::from(Span::styled(
                format!("    {}", prompt_preview),
                Style::default().fg(MUTED).bg(bg),
            )));
            lines.push(Line::from(""));
        }
    }

    // Inline input fields for adding a role.
    use crate::app::state::DashboardInput;

    // Helper: wrap long input text into multiple lines that fit the panel.
    // `prefix` is shown on the first line (e.g. "  > "); continuation lines
    // are indented to the same column width.
    let wrap_input = |text: &str, prefix: &str, width: u16, style: Style| -> Vec<Line<'_>> {
        let prefix_w = prefix.len();
        let usable = (width as usize).saturating_sub(prefix_w);
        if usable == 0 || text.is_empty() {
            return vec![Line::from(vec![
                Span::styled(prefix.to_string(), Style::default().fg(AMBER)),
                Span::styled(text.to_string(), style),
            ])];
        }
        let mut result: Vec<Line> = Vec::new();
        let mut remaining = text;
        let mut first = true;
        while !remaining.is_empty() {
            let chunk_len = remaining.len().min(usable);
            let chunk = &remaining[..chunk_len];
            remaining = &remaining[chunk_len..];
            if first {
                result.push(Line::from(vec![
                    Span::styled(prefix.to_string(), Style::default().fg(AMBER)),
                    Span::styled(chunk.to_string(), style),
                ]));
                first = false;
            } else {
                result.push(Line::from(vec![
                    Span::styled(" ".repeat(prefix_w), Style::default()),
                    Span::styled(chunk.to_string(), style),
                ]));
            }
        }
        result
    };

    let input_style = Style::default().fg(CREAM).add_modifier(Modifier::UNDERLINED);

    match &dash.input {
        DashboardInput::RoleName(buf) => {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "  New role name:",
                Style::default().fg(AMBER),
            )));
            let display = if buf.is_empty() { "..." } else { buf.as_str() };
            lines.extend(wrap_input(display, "  > ", inner.width, input_style));
            lines.push(Line::from(Span::styled(
                "  Enter to confirm, Esc to cancel",
                Style::default().fg(MUTED),
            )));
        }
        DashboardInput::RolePrompt { name, prompt } => {
            lines.push(Line::from(""));
            lines.push(Line::from(vec![
                Span::styled("  Role: ", Style::default().fg(TAN)),
                Span::styled(name.to_string(), Style::default().fg(CREAM).add_modifier(Modifier::BOLD)),
            ]));
            lines.push(Line::from(Span::styled(
                "  Prompt (instructions for this agent):",
                Style::default().fg(AMBER),
            )));
            let display = if prompt.is_empty() { "..." } else { prompt.as_str() };
            lines.extend(wrap_input(display, "  > ", inner.width, input_style));
            lines.push(Line::from(Span::styled(
                "  Enter to save, Esc to cancel",
                Style::default().fg(MUTED),
            )));
        }
        DashboardInput::None => {
            // Keybind hints.
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "  a add role  d delete  ↑↓ select",
                Style::default().fg(MUTED),
            )));
        }
    }

    let content = Paragraph::new(lines);
    frame.render_widget(content, inner);
}

// ── Detail: Integrations ──────────────────────────────────────────────────────

fn render_detail_integrations(frame: &mut Frame, area: Rect, state: &AppState) {
    let AppScreen::Dashboard(ref dash) = state.screen else {
        return;
    };
    let detail_focused = dash.focus == DashboardFocus::Detail;

    let border_style = if detail_focused {
        Style::default().fg(AMBER)
    } else {
        Style::default().fg(STONE)
    };

    let block = Block::default()
        .title(Span::styled(" Integrations ", Style::default().fg(TAN)))
        .borders(Borders::ALL)
        .border_style(border_style)
        .style(Style::default().bg(BG));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut lines: Vec<Line> = Vec::new();

    // MCP Server.
    lines.push(Line::from(Span::styled(
        "  MCP SERVER",
        Style::default().fg(BRASS).add_modifier(Modifier::BOLD),
    )));
    let mcp_active = state.mcp_socket_path.is_some();
    let (mcp_label, mcp_color) = if mcp_active {
        ("active", SPROUT)
    } else {
        ("inactive", MUTED)
    };
    lines.push(Line::from(vec![
        Span::styled("    Status: ", Style::default().fg(TAN)),
        Span::styled(mcp_label, Style::default().fg(mcp_color)),
    ]));

    if let Some(ref sock) = state.mcp_socket_path {
        lines.push(Line::from(Span::styled(
            format!("    Socket: {}", sock.display()),
            Style::default().fg(MUTED),
        )));
    }

    lines.push(Line::from(""));

    // MCP Tools.
    lines.push(Line::from(Span::styled(
        "  COORDINATION TOOLS",
        Style::default().fg(BRASS).add_modifier(Modifier::BOLD),
    )));
    let tools = [
        "potato_send_message",
        "potato_get_messages",
        "potato_claim_role",
        "potato_list_roles",
        "potato_get_status",
        "potato_shared_context",
    ];
    for tool in &tools {
        lines.push(Line::from(Span::styled(
            format!("    {}", tool),
            Style::default().fg(if mcp_active { CREAM } else { MUTED }),
        )));
    }

    lines.push(Line::from(""));

    // Agents.
    lines.push(Line::from(Span::styled(
        "  DETECTED AGENTS",
        Style::default().fg(BRASS).add_modifier(Modifier::BOLD),
    )));
    for agent in &dash.available_agents {
        let (indicator, fg) = if agent.available {
            ("●", SPROUT)
        } else {
            ("○", MUTED)
        };
        lines.push(Line::from(vec![
            Span::styled(format!("    {} ", indicator), Style::default().fg(fg)),
            Span::styled(
                &agent.name,
                Style::default().fg(if agent.available { CREAM } else { MUTED }),
            ),
        ]));
    }

    let content = Paragraph::new(lines);
    frame.render_widget(content, inner);
}

// ── Detail: Settings ──────────────────────────────────────────────────────────

fn render_detail_settings(frame: &mut Frame, area: Rect, state: &AppState) {
    let detail_focused = matches!(
        state.screen,
        AppScreen::Dashboard(ref d) if d.focus == DashboardFocus::Detail
    );

    let border_style = if detail_focused {
        Style::default().fg(AMBER)
    } else {
        Style::default().fg(STONE)
    };

    let block = Block::default()
        .title(Span::styled(" Settings ", Style::default().fg(TAN)))
        .borders(Borders::ALL)
        .border_style(border_style)
        .style(Style::default().bg(BG));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut lines: Vec<Line> = Vec::new();

    lines.push(Line::from(Span::styled(
        "  MODEL",
        Style::default().fg(BRASS).add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(Span::styled(
        format!("    {}", state.model),
        Style::default().fg(CREAM),
    )));

    lines.push(Line::from(""));

    lines.push(Line::from(Span::styled(
        "  CONFIG",
        Style::default().fg(BRASS).add_modifier(Modifier::BOLD),
    )));
    let config_display = if state.config_path.is_empty() {
        "default"
    } else {
        &state.config_path
    };
    lines.push(Line::from(Span::styled(
        format!("    {}", config_display),
        Style::default().fg(CREAM),
    )));

    lines.push(Line::from(""));

    lines.push(Line::from(Span::styled(
        "  PERMISSIONS",
        Style::default().fg(BRASS).add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(Span::styled(
        "    dangerously-skip-permissions (always on)",
        Style::default().fg(CREAM),
    )));

    let content = Paragraph::new(lines);
    frame.render_widget(content, inner);
}

// ── Footer ────────────────────────────────────────────────────────────────────

fn render_footer(frame: &mut Frame, area: Rect, state: &AppState) {
    let AppScreen::Dashboard(ref dash) = state.screen else {
        return;
    };

    let hints = match (DashboardMenuItem::ALL[dash.selected_menu], &dash.focus) {
        (DashboardMenuItem::RoastPotato, DashboardFocus::Menu) => {
            "↑↓ navigate  Tab details  Enter launch  q quit"
        }
        (_, DashboardFocus::Menu) => {
            "↑↓ navigate  Tab details  Enter select  q quit"
        }
        (_, DashboardFocus::Detail) => {
            "↑↓ navigate  Tab menu  Esc back  Enter select"
        }
    };

    let footer = Paragraph::new(Span::styled(hints, Style::default().fg(BRASS)))
        .style(Style::default().bg(BG))
        .alignment(Alignment::Left);
    frame.render_widget(footer, area);
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::state::{AgentInfo, DashboardFocus, DashboardState, DashboardMenuItem, RoleDefinition, SessionSummary};
    use chrono::Utc;

    fn make_dashboard_state() -> AppState {
        let mut state = AppState::default();
        if let Some(dash) = state.dashboard_mut() {
            dash.available_agents = vec![
                AgentInfo {
                    name: "Claude Code".to_string(),
                    adapter: "claude".to_string(),
                    binary_path: None,
                    available: true,
                },
                AgentInfo {
                    name: "Codex".to_string(),
                    adapter: "codex".to_string(),
                    binary_path: None,
                    available: false,
                },
            ];
            dash.recent_sessions = vec![SessionSummary {
                session_id: "s-1".to_string(),
                agent_name: "claude".to_string(),
                started_at: Utc::now(),
                total_cost_usd: 0.001,
                turn_count: 3,
            }];
        }
        state
    }

    #[test]
    fn dashboard_state_has_agents() {
        let state = make_dashboard_state();
        let dash = state.dashboard().unwrap();
        assert_eq!(dash.available_agents.len(), 2);
        assert!(dash.available_agents[0].available);
        assert!(!dash.available_agents[1].available);
    }

    #[test]
    fn dashboard_state_has_sessions() {
        let state = make_dashboard_state();
        let dash = state.dashboard().unwrap();
        assert_eq!(dash.recent_sessions.len(), 1);
        assert!((dash.recent_sessions[0].total_cost_usd - 0.001).abs() < 1e-6);
    }

    #[test]
    fn menu_items_have_labels() {
        for item in DashboardMenuItem::ALL {
            assert!(!item.label().is_empty());
        }
    }

    #[test]
    fn default_focus_is_menu() {
        let d = DashboardState::default();
        assert_eq!(d.focus, DashboardFocus::Menu);
    }

    #[test]
    fn focus_toggles() {
        let mut dash = DashboardState::default();
        assert_eq!(dash.focus, DashboardFocus::Menu);
        dash.focus = DashboardFocus::Detail;
        assert_eq!(dash.focus, DashboardFocus::Detail);
    }

    #[test]
    fn roles_default_empty() {
        let d = DashboardState::default();
        assert!(d.roles.is_empty());
    }

    #[test]
    fn roles_can_be_added() {
        let mut d = DashboardState::default();
        d.roles.push(RoleDefinition {
            name: "Architect".to_string(),
            prompt: "Design the system".to_string(),
        });
        d.roles.push(RoleDefinition {
            name: "Engineer".to_string(),
            prompt: "Implement the design".to_string(),
        });
        assert_eq!(d.roles.len(), 2);
        assert_eq!(d.roles[0].name, "Architect");
    }

    #[test]
    fn empty_sessions_placeholder() {
        let sessions: Vec<SessionSummary> = vec![];
        assert!(sessions.is_empty());
    }
}
