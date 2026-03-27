//! Dashboard screen — agent picker and recent sessions list.
//!
//! Layout:
//! ```
//! ┌──────────────────────────────────────────────┐
//! │              🥔  Potato                       │  title
//! ├───────────────────┬──────────────────────────┤
//! │  Agents           │  Recent Sessions         │
//! │  ● Claude Code    │  [claude] 2024-01 $0.01  │
//! │    Codex (n/a)    │  [codex]  2024-01 $0.00  │
//! ├───────────────────┴──────────────────────────┤
//! │  [Enter] launch  [Tab] switch pane  [q] quit │  footer
//! └──────────────────────────────────────────────┘
//! ```

use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
};

use crate::app::state::{AppState, AppScreen, DashboardFocus};
use crate::ui::theme::{AMBER, BG, BROWN, CHARCOAL, CREAM, RUST_RED, SOIL, TAN};

// ── Entry point ───────────────────────────────────────────────────────────────

/// Render the full dashboard screen.
pub fn render_dashboard(frame: &mut Frame, area: Rect, state: &AppState) {
    let AppScreen::Dashboard(ref dash) = state.screen else { return };

    // Outer block fills the whole area with the background colour.
    let outer = Block::default()
        .style(Style::default().bg(BG));
    frame.render_widget(outer, area);

    // Vertical split: title, content, footer.
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),  // title bar
            Constraint::Min(0),     // main content
            Constraint::Length(1),  // footer
        ])
        .split(area);

    render_title(frame, rows[0]);
    render_content(frame, rows[1], state);
    render_footer(frame, rows[2], dash.focus == DashboardFocus::AgentList);
}

// ── Title ─────────────────────────────────────────────────────────────────────

fn render_title(frame: &mut Frame, area: Rect) {
    let title = Paragraph::new("🥔  Potato")
        .alignment(Alignment::Center)
        .style(Style::default().fg(AMBER).bg(CHARCOAL).add_modifier(Modifier::BOLD))
        .block(Block::default().borders(Borders::BOTTOM).border_style(Style::default().fg(SOIL)));
    frame.render_widget(title, area);
}

// ── Content (two-column split) ────────────────────────────────────────────────

fn render_content(frame: &mut Frame, area: Rect, state: &AppState) {
    let AppScreen::Dashboard(ref dash) = state.screen else { return };

    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(35), Constraint::Percentage(65)])
        .split(area);

    render_agent_list(frame, cols[0], dash.available_agents.as_slice(), dash.selected_agent, dash.focus == DashboardFocus::AgentList);
    render_session_list(frame, cols[1], dash.recent_sessions.as_slice(), dash.selected_session, dash.focus == DashboardFocus::SessionList);
}

// ── Agent list ────────────────────────────────────────────────────────────────

fn render_agent_list(
    frame: &mut Frame,
    area: Rect,
    agents: &[crate::app::state::AgentInfo],
    selected: usize,
    focused: bool,
) {
    let border_style = if focused {
        Style::default().fg(AMBER)
    } else {
        Style::default().fg(SOIL)
    };

    let block = Block::default()
        .title(Span::styled(" Agents ", Style::default().fg(TAN)))
        .borders(Borders::ALL)
        .border_style(border_style)
        .style(Style::default().bg(BG));

    let items: Vec<ListItem> = agents
        .iter()
        .enumerate()
        .map(|(i, agent)| {
            let (indicator, fg) = if agent.available {
                ("● ", AMBER)   // green-ish (amber) sprout = available
            } else {
                ("○ ", SOIL)    // muted = not available
            };

            let style = if i == selected && focused {
                Style::default().fg(CREAM).bg(CHARCOAL).add_modifier(Modifier::BOLD)
            } else if i == selected {
                Style::default().fg(CREAM).bg(CHARCOAL)
            } else {
                Style::default().fg(fg)
            };

            let line = Line::from(vec![
                Span::styled(indicator, Style::default().fg(fg)),
                Span::styled(agent.name.clone(), style),
            ]);
            ListItem::new(line)
        })
        .collect();

    let mut list_state = ListState::default();
    if !agents.is_empty() {
        list_state.select(Some(selected.min(agents.len().saturating_sub(1))));
    }

    let list = List::new(items)
        .block(block)
        .highlight_style(Style::default().fg(CREAM).bg(CHARCOAL).add_modifier(Modifier::BOLD));

    frame.render_stateful_widget(list, area, &mut list_state);
}

// ── Session list ──────────────────────────────────────────────────────────────

fn render_session_list(
    frame: &mut Frame,
    area: Rect,
    sessions: &[crate::app::state::SessionSummary],
    selected: usize,
    focused: bool,
) {
    let border_style = if focused {
        Style::default().fg(AMBER)
    } else {
        Style::default().fg(SOIL)
    };

    let block = Block::default()
        .title(Span::styled(" Recent Sessions ", Style::default().fg(TAN)))
        .borders(Borders::ALL)
        .border_style(border_style)
        .style(Style::default().bg(BG));

    if sessions.is_empty() {
        let placeholder = Paragraph::new("\n  No recent sessions.\n  Press Enter on an agent to start one.")
            .style(Style::default().fg(SOIL))
            .block(block);
        frame.render_widget(placeholder, area);
        return;
    }

    let items: Vec<ListItem> = sessions
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let date = s.started_at.format("%Y-%m-%d %H:%M").to_string();
            let cost = if s.total_cost_usd > 0.0 {
                format!("${:.3}", s.total_cost_usd)
            } else {
                "—".to_string()
            };

            let is_selected = i == selected;
            let style = if is_selected && focused {
                Style::default().fg(CREAM).bg(CHARCOAL).add_modifier(Modifier::BOLD)
            } else if is_selected {
                Style::default().fg(CREAM).bg(CHARCOAL)
            } else {
                Style::default().fg(TAN)
            };

            let line = Line::from(vec![
                Span::styled(format!("  [{:<10}] ", s.agent_name), Style::default().fg(BROWN)),
                Span::styled(date, style),
                Span::styled(format!("  {}", cost), Style::default().fg(AMBER)),
            ]);
            ListItem::new(line)
        })
        .collect();

    let mut list_state = ListState::default();
    list_state.select(Some(selected.min(sessions.len().saturating_sub(1))));

    let list = List::new(items)
        .block(block)
        .highlight_style(Style::default().fg(CREAM).bg(CHARCOAL).add_modifier(Modifier::BOLD));

    frame.render_stateful_widget(list, area, &mut list_state);
}

// ── Footer ────────────────────────────────────────────────────────────────────

fn render_footer(frame: &mut Frame, area: Rect, _agents_focused: bool) {
    let sep = Span::styled("  │  ", Style::default().fg(SOIL));
    let line = Line::from(vec![
        Span::styled(" [Enter] launch ", Style::default().fg(AMBER)),
        sep.clone(),
        Span::styled("[Tab] switch pane ", Style::default().fg(TAN)),
        sep.clone(),
        Span::styled("[q] quit ", Style::default().fg(RUST_RED)),
    ]);
    let footer = Paragraph::new(line)
        .style(Style::default().bg(CHARCOAL))
        .alignment(Alignment::Left);
    frame.render_widget(footer, area);
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::state::{AgentInfo, DashboardState, SessionSummary};
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
            dash.recent_sessions = vec![
                SessionSummary {
                    session_id: "s-1".to_string(),
                    agent_name: "claude".to_string(),
                    started_at: Utc::now(),
                    total_cost_usd: 0.001,
                    turn_count: 3,
                },
            ];
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
    fn focus_cycles() {
        let mut dash = DashboardState::default();
        assert_eq!(dash.focus, DashboardFocus::AgentList);
        dash.focus = DashboardFocus::SessionList;
        assert_eq!(dash.focus, DashboardFocus::SessionList);
    }
}
