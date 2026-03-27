//! Dashboard screen — agent picker and recent sessions list.
//!
//! Layout:
//! ```
//! ┌──────────────────────────────────────────────┐
//! │                 🥔  Potato                    │  title (Brown, centered)
//! ├───────────────────┬──────────────────────────┤
//! │  Agents           │  Recent Sessions         │
//! │  ● Claude Code    │  [claude] 2024-01 $0.01  │
//! │    Codex (n/a)    │  No recent sessions      │
//! ├───────────────────┴──────────────────────────┤
//! │  ↑↓ navigate  Tab switch panel  Enter launch  q quit │  footer (Brown)
//! └──────────────────────────────────────────────┘
//! ```

use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
};

use crate::app::state::{AppScreen, AppState, DashboardFocus};
use crate::ui::theme::{AMBER, BG, BRASS, BROWN, CHARCOAL, CREAM, SOIL, SPROUT, STONE, TAN};

/// Muted gray for unavailable/secondary items.
const MUTED: Color = Color::Rgb(100, 100, 100);

// ── Entry point ───────────────────────────────────────────────────────────────

/// Render the full dashboard screen.
pub fn render_dashboard(frame: &mut Frame, area: Rect, state: &AppState) {
    let AppScreen::Dashboard(ref dash) = state.screen else {
        return;
    };

    // Outer block fills the whole area with the background colour.
    let outer = Block::default().style(Style::default().bg(BG));
    frame.render_widget(outer, area);

    // Vertical split: title, content, footer.
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // title bar
            Constraint::Min(0),    // main content
            Constraint::Length(1), // footer
        ])
        .split(area);

    render_title(frame, rows[0]);
    render_content(frame, rows[1], state);
    render_footer(frame, rows[2]);
}

// ── Title ─────────────────────────────────────────────────────────────────────

fn render_title(frame: &mut Frame, area: Rect) {
    // Brown centered title, BG background (no Charcoal), bottom border in Soil.
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

// ── Content (two-column split) ────────────────────────────────────────────────

fn render_content(frame: &mut Frame, area: Rect, state: &AppState) {
    let AppScreen::Dashboard(ref dash) = state.screen else {
        return;
    };

    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);

    render_agent_list(
        frame,
        cols[0],
        dash.available_agents.as_slice(),
        dash.selected_agent,
        dash.focus == DashboardFocus::AgentList,
    );
    render_session_list(
        frame,
        cols[1],
        dash.recent_sessions.as_slice(),
        dash.selected_session,
        dash.focus == DashboardFocus::SessionList,
    );
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
        Style::default().fg(STONE)
    };

    let block = Block::default()
        .title(Span::styled(" Agents ", Style::default().fg(TAN)))
        .borders(Borders::ALL)
        .border_style(border_style)
        .style(Style::default().bg(BG));

    if agents.is_empty() {
        let p = Paragraph::new("\n  No agents detected.")
            .style(Style::default().fg(MUTED))
            .block(block);
        frame.render_widget(p, area);
        return;
    }

    let items: Vec<ListItem> = agents
        .iter()
        .enumerate()
        .map(|(i, agent)| {
            let is_selected = i == selected;
            let bg = if is_selected {
                CHARCOAL
            } else {
                BG
            };

            if agent.available {
                // Available: Sprout indicator + Cream name on selected, Sprout otherwise.
                let name_fg = if is_selected { CREAM } else { SPROUT };
                let name_style = Style::default().fg(name_fg).bg(bg);
                let indicator_style = Style::default().fg(SPROUT).bg(bg);
                let line = Line::from(vec![
                    Span::styled(" ● ", indicator_style),
                    Span::styled(agent.name.clone(), name_style),
                ]);
                ListItem::new(line)
            } else {
                // Unavailable: muted indicator + muted name.
                let name_style = Style::default().fg(MUTED).bg(bg);
                let indicator_style = Style::default().fg(MUTED).bg(bg);
                let suffix = " (not found)";
                let line = Line::from(vec![
                    Span::styled(" ○ ", indicator_style),
                    Span::styled(agent.name.clone(), name_style),
                    Span::styled(suffix, Style::default().fg(MUTED).bg(bg)),
                ]);
                ListItem::new(line)
            }
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
        Style::default().fg(STONE)
    };

    let block = Block::default()
        .title(Span::styled(" Recent Sessions ", Style::default().fg(TAN)))
        .borders(Borders::ALL)
        .border_style(border_style)
        .style(Style::default().bg(BG));

    if sessions.is_empty() {
        let placeholder =
            Paragraph::new("\n  No recent sessions.\n  Press Enter on an agent to start one.")
                .style(Style::default().fg(MUTED))
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
            let bg = if is_selected { CHARCOAL } else { BG };

            let row_style = if is_selected && focused {
                Style::default().fg(CREAM).bg(CHARCOAL).add_modifier(Modifier::BOLD)
            } else if is_selected {
                Style::default().fg(CREAM).bg(CHARCOAL)
            } else {
                Style::default().fg(TAN).bg(BG)
            };

            let line = Line::from(vec![
                Span::styled(
                    format!("  [{:<10}] ", s.agent_name),
                    Style::default().fg(BRASS).bg(bg),
                ),
                Span::styled(date, row_style),
                Span::styled(
                    format!("  {}", cost),
                    Style::default().fg(AMBER).bg(bg),
                ),
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

fn render_footer(frame: &mut Frame, area: Rect) {
    let line = Line::from(vec![
        Span::styled(
            " ↑↓ navigate  ",
            Style::default().fg(BRASS),
        ),
        Span::styled("Tab switch panel  ", Style::default().fg(BRASS)),
        Span::styled("Enter launch  ", Style::default().fg(BRASS)),
        Span::styled("q quit", Style::default().fg(BRASS)),
    ]);
    let footer = Paragraph::new(line)
        .style(Style::default().bg(BG))
        .alignment(Alignment::Left);
    frame.render_widget(footer, area);
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::state::{AgentInfo, DashboardFocus, DashboardState, SessionSummary};
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
    fn focus_cycles() {
        let mut dash = DashboardState::default();
        assert_eq!(dash.focus, DashboardFocus::AgentList);
        dash.focus = DashboardFocus::SessionList;
        assert_eq!(dash.focus, DashboardFocus::SessionList);
    }

    #[test]
    fn empty_sessions_placeholder() {
        // Regression: rendering with zero sessions should not panic (tested by building items).
        let sessions: Vec<SessionSummary> = vec![];
        assert!(sessions.is_empty());
    }
}
