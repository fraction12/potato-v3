//! Dashboard screen — Option B layout with left menu rail and right detail pane.
//!
//! Layout:
//! ```text
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
//! │  ↑↓ navigate  Tab switch  Enter select  Ctrl+Q quit                      │
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
            Constraint::Length(4), // title bar (name + subtitle)
            Constraint::Min(0),    // main content
            Constraint::Length(1), // footer
        ])
        .split(area);

    render_title(frame, rows[0]);
    render_content(frame, rows[1], state);
    render_footer(frame, rows[2], state);
}

// ── Title ─────────────────────────────────────────────────────────────────────

fn render_title(frame: &mut Frame, area: Rect) {
    // Outer block with bottom border.
    let block = Block::default()
        .borders(Borders::BOTTOM)
        .border_style(Style::default().fg(STONE))
        .style(Style::default().bg(BG));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    // Centered title.
    let title = Paragraph::new("🥔  POTATO")
        .alignment(Alignment::Center)
        .style(
            Style::default()
                .fg(BRASS)
                .bg(BG)
                .add_modifier(Modifier::BOLD),
        );
    frame.render_widget(title, inner);

    // Subtitle below title.
    if inner.height >= 2 {
        let subtitle_area = Rect {
            x: inner.x,
            y: inner.y + 1,
            width: inner.width,
            height: 1,
        };
        let subtitle = Paragraph::new("Personal Orchestration Tool for Agentic Task Operations")
            .alignment(Alignment::Center)
            .style(Style::default().fg(STONE).bg(BG));
        frame.render_widget(subtitle, subtitle_area);
    }

    // Right-aligned version label.
    if inner.width > 10 {
        let ver = Paragraph::new("v0.1.0")
            .alignment(Alignment::Right)
            .style(Style::default().fg(STONE).bg(BG));
        frame.render_widget(ver, inner);
    }
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

fn render_menu(frame: &mut Frame, area: Rect, dash: &crate::app::state::DashboardState) {
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

    // Menu item icon prefixes.
    let menu_icons = ["▶", "👤", "⚡", "⚙"];

    let mut raw_items: Vec<ListItem> = Vec::new();
    for (i, item) in DashboardMenuItem::ALL.iter().enumerate() {
        // Divider after the first item (Roast Potato is the primary action).
        if i == 1 {
            raw_items.push(ListItem::new(Line::from(Span::styled(
                "  ─────────────────────",
                Style::default().fg(STONE).bg(BG),
            ))));
        }

        let is_selected = i == dash.selected_menu;
        let bg = if is_selected { CHARCOAL } else { BG };
        let fg = if is_selected { CREAM } else { TAN };
        let icon = menu_icons.get(i).copied().unwrap_or("");
        let style = if is_selected {
            Style::default().fg(fg).bg(bg).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(fg).bg(bg)
        };
        raw_items.push(ListItem::new(Line::from(Span::styled(
            format!("  {} {}", icon, item.label()),
            style,
        ))));
        // 1-line padding between items (except after the last).
        if i < DashboardMenuItem::ALL.len() - 1 {
            raw_items.push(ListItem::new(Line::from(Span::styled(
                "",
                Style::default().bg(BG),
            ))));
        }
    }
    let items = raw_items;

    // Map logical selected_menu index to visual list index.
    // Layout: item0, blank, divider, item1, blank, item2, blank, item3
    // indices:   0     1      2       3      4       5      6      7
    let visual_index = if dash.selected_menu == 0 {
        0
    } else {
        // +2 for the divider row (before item 1), +1 per blank between items
        2 + (dash.selected_menu - 1) * 2 + 1
    };
    let mut list_state = ListState::default();
    list_state.select(Some(visual_index));

    let list = List::new(items).block(block).highlight_style(
        Style::default()
            .fg(CREAM)
            .bg(CHARCOAL)
            .add_modifier(Modifier::BOLD),
    );

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

    // Helper: draw a subtle section underline.
    let section_under = |width: u16| -> Line<'static> {
        let dashes = "─".repeat((width as usize).saturating_sub(2).min(40));
        Line::from(Span::styled(
            format!("  {}", dashes),
            Style::default().fg(STONE),
        ))
    };

    // Agents summary.
    lines.push(Line::from(Span::styled(
        "  AGENTS",
        Style::default().fg(BRASS).add_modifier(Modifier::BOLD),
    )));
    lines.push(section_under(inner.width));
    for agent in &dash.available_agents {
        let (indicator, fg) = if agent.available {
            ("●", SPROUT)
        } else {
            ("○", MUTED)
        };
        let status = if agent.available {
            "ready"
        } else {
            "not found"
        };
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
    lines.push(section_under(inner.width));
    if dash.roles.is_empty() {
        lines.push(Line::from(Span::styled(
            "    No roles defined. Agents will self-organize.",
            Style::default().fg(MUTED),
        )));
    } else {
        for (i, role) in dash.roles.iter().enumerate() {
            lines.push(Line::from(vec![
                Span::styled(format!("    Pane {} ", i + 1), Style::default().fg(TAN)),
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
    lines.push(section_under(inner.width));
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
        "  [ Enter to Launch ]",
        Style::default().fg(AMBER).add_modifier(Modifier::BOLD),
    )));

    lines.push(Line::from(""));
    lines.push(Line::from(""));

    // Recent sessions.
    lines.push(Line::from(Span::styled(
        "  RECENT SESSIONS",
        Style::default().fg(BRASS).add_modifier(Modifier::BOLD),
    )));
    lines.push(section_under(inner.width));
    // Column headers.
    lines.push(Line::from(Span::styled(
        "  Agent           Date        Cost",
        Style::default().fg(STONE),
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
                Span::styled(format!("  {}", cost), Style::default().fg(AMBER).bg(bg)),
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

    // Helper: wrap text at a given width, returning indented lines.
    let wrap_text = |text: &str, indent: usize, width: u16, style: Style| -> Vec<Line<'_>> {
        let usable = (width as usize).saturating_sub(indent);
        if usable == 0 {
            return vec![Line::from(Span::styled(text.to_string(), style))];
        }
        let pad = " ".repeat(indent);
        let mut result: Vec<Line> = Vec::new();
        let mut remaining = text;
        while !remaining.is_empty() {
            let char_count = remaining.chars().count();
            let chunk_chars = char_count.min(usable);
            // Find the byte offset of the chunk_chars-th character.
            let chunk_byte_end = remaining
                .char_indices()
                .nth(chunk_chars)
                .map(|(i, _)| i)
                .unwrap_or(remaining.len());
            // Try to break at a space if we're not at the end.
            let break_byte = if chunk_byte_end < remaining.len() {
                remaining[..chunk_byte_end]
                    .rfind(' ')
                    .map(|p| p + 1)
                    .unwrap_or(chunk_byte_end)
            } else {
                chunk_byte_end
            };
            let chunk = &remaining[..break_byte];
            remaining = &remaining[break_byte..];
            result.push(Line::from(Span::styled(
                format!("{}{}", pad, chunk.trim_end()),
                style,
            )));
        }
        if result.is_empty() {
            result.push(Line::from(Span::styled(
                format!("{}...", " ".repeat(indent)),
                style,
            )));
        }
        result
    };

    if dash.roles.is_empty() {
        // Show the defaults: one pane, no role prompt, self-organizing.
        lines.push(Line::from(Span::styled(
            "  Defaults (no custom roles):",
            Style::default().fg(TAN).add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "  Pane 1 — Claude",
            Style::default().fg(CREAM),
        )));
        lines.push(Line::from(Span::styled(
            "    No role prompt. Agent starts with a blank slate",
            Style::default().fg(MUTED),
        )));
        lines.push(Line::from(Span::styled(
            "    and self-organizes based on the project context",
            Style::default().fg(MUTED),
        )));
        lines.push(Line::from(Span::styled(
            "    and MCP coordination tools.",
            Style::default().fg(MUTED),
        )));
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "  Press F2 to add a role and customise behaviour.",
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
                    role.name.clone(),
                    Style::default()
                        .fg(CREAM)
                        .bg(bg)
                        .add_modifier(Modifier::BOLD),
                ),
            ]));
            // Show full instructions, wrapped.
            let prompt_style = Style::default().fg(MUTED).bg(bg);
            lines.extend(wrap_text(&role.prompt, 4, inner.width, prompt_style));
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
            let char_count = remaining.chars().count();
            let chunk_chars = char_count.min(usable);
            let chunk_byte_end = remaining
                .char_indices()
                .nth(chunk_chars)
                .map(|(i, _)| i)
                .unwrap_or(remaining.len());
            let chunk = &remaining[..chunk_byte_end];
            remaining = &remaining[chunk_byte_end..];
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

    let input_style = Style::default()
        .fg(CREAM)
        .add_modifier(Modifier::UNDERLINED);

    match &dash.input {
        DashboardInput::RoleName(buf) => {
            lines.push(Line::from(""));
            // Bordered input box for role name.
            let box_width = (inner.width as usize).saturating_sub(6).min(48);
            let title_str = "─ Role Name ";
            let fill_count = box_width.saturating_sub(title_str.len() + 2);
            let top = format!("  ┌{}{}┐", title_str, "─".repeat(fill_count));
            let content = if buf.is_empty() {
                "Type here...".to_string()
            } else {
                buf.clone()
            };
            let padded = format!("{:<width$}", content, width = box_width);
            let mid_content: String = padded.chars().take(box_width).collect();
            let mid = format!("  │ {} │", mid_content);
            let bot = format!("  └{}┘", "─".repeat(box_width + 2));
            let content_style = if buf.is_empty() {
                Style::default().fg(MUTED)
            } else {
                Style::default()
                    .fg(CREAM)
                    .add_modifier(Modifier::UNDERLINED)
            };
            lines.push(Line::from(Span::styled(top, Style::default().fg(AMBER))));
            lines.push(Line::from(vec![
                Span::styled("  │ ", Style::default().fg(AMBER)),
                Span::styled(content, content_style),
                Span::styled(
                    format!(
                        "{} │",
                        " ".repeat(box_width.saturating_sub(if buf.is_empty() {
                            12
                        } else {
                            buf.len().min(box_width)
                        }))
                    ),
                    Style::default().fg(AMBER),
                ),
            ]));
            lines.push(Line::from(Span::styled(bot, Style::default().fg(AMBER))));
            lines.push(Line::from(Span::styled(
                "  Enter to confirm, Esc to cancel",
                Style::default().fg(MUTED),
            )));
        }
        DashboardInput::RolePrompt { name, prompt } => {
            lines.push(Line::from(""));
            lines.push(Line::from(vec![
                Span::styled("  Role: ", Style::default().fg(TAN)),
                Span::styled(
                    name.to_string(),
                    Style::default().fg(CREAM).add_modifier(Modifier::BOLD),
                ),
            ]));
            // Bordered input box for role prompt.
            let box_width = (inner.width as usize).saturating_sub(6).min(48);
            let title_str = "─ Instructions ";
            let fill_count = box_width.saturating_sub(title_str.len() + 2);
            let top = format!("  ┌{}{}┐", title_str, "─".repeat(fill_count));
            let content = if prompt.is_empty() {
                "Type here...".to_string()
            } else {
                prompt.clone()
            };
            let content_len = if prompt.is_empty() {
                12
            } else {
                prompt.len().min(box_width)
            };
            let bot = format!("  └{}┘", "─".repeat(box_width + 2));
            let content_style = if prompt.is_empty() {
                Style::default().fg(MUTED)
            } else {
                Style::default()
                    .fg(CREAM)
                    .add_modifier(Modifier::UNDERLINED)
            };
            lines.push(Line::from(Span::styled(top, Style::default().fg(AMBER))));
            lines.push(Line::from(vec![
                Span::styled("  │ ", Style::default().fg(AMBER)),
                Span::styled(content, content_style),
                Span::styled(
                    format!("{} │", " ".repeat(box_width.saturating_sub(content_len))),
                    Style::default().fg(AMBER),
                ),
            ]));
            lines.push(Line::from(Span::styled(bot, Style::default().fg(AMBER))));
            lines.push(Line::from(Span::styled(
                "  Enter to save, Esc to cancel",
                Style::default().fg(MUTED),
            )));
        }
        DashboardInput::None => {
            // Keybind hints.
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "  F2 add role  F3 delete  ↑↓ select",
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

    // GIT section.
    lines.push(Line::from(Span::styled(
        "  GIT",
        Style::default().fg(BRASS).add_modifier(Modifier::BOLD),
    )));
    let git = &state.git_snapshot;
    let (git_status_label, git_status_color) = if git.is_repo {
        ("tracking", SPROUT)
    } else {
        ("not a repo", MUTED)
    };
    lines.push(Line::from(vec![
        Span::styled("    Status:  ", Style::default().fg(TAN)),
        Span::styled(git_status_label, Style::default().fg(git_status_color)),
    ]));
    if git.is_repo {
        let branch_display = if git.current_branch.is_empty() {
            "unknown".to_string()
        } else {
            git.current_branch.clone()
        };
        lines.push(Line::from(vec![
            Span::styled("    Branch:  ", Style::default().fg(TAN)),
            Span::styled(branch_display, Style::default().fg(CREAM)),
        ]));
        let (dirty_label, dirty_color) = if git.dirty_count == 0 {
            ("clean".to_string(), SPROUT)
        } else {
            (format!("{} files", git.dirty_count), AMBER)
        };
        lines.push(Line::from(vec![
            Span::styled("    Dirty:   ", Style::default().fg(TAN)),
            Span::styled(dirty_label, Style::default().fg(dirty_color)),
        ]));
        lines.push(Line::from(vec![
            Span::styled("    Commits: ", Style::default().fg(TAN)),
            Span::styled(
                format!("{} recent", git.recent_commits.len()),
                Style::default().fg(CREAM),
            ),
        ]));
    }

    lines.push(Line::from(""));

    // OPENSPEC section.
    lines.push(Line::from(Span::styled(
        "  OPENSPEC",
        Style::default().fg(BRASS).add_modifier(Modifier::BOLD),
    )));
    let (os_status_label, os_status_color) = if state.openspec_snapshot.cli_available {
        ("active", SPROUT)
    } else {
        ("not found", MUTED)
    };
    lines.push(Line::from(vec![
        Span::styled("    Status:     ", Style::default().fg(TAN)),
        Span::styled(os_status_label, Style::default().fg(os_status_color)),
    ]));
    if state.openspec_snapshot.cli_available {
        let change_count = state.openspec_snapshot.changes.len();
        lines.push(Line::from(vec![
            Span::styled("    Changes:    ", Style::default().fg(TAN)),
            Span::styled(format!("{change_count}"), Style::default().fg(CREAM)),
        ]));
    }

    lines.push(Line::from(""));

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

/// Build all content lines for the Settings detail panel.
///
/// Extracted so it can be tested without a `Frame`.
pub(crate) fn build_settings_lines(state: &AppState) -> Vec<Line<'static>> {
    let dash = match &state.screen {
        AppScreen::Dashboard(d) => d,
        _ => return Vec::new(),
    };
    let cfg = &state.config;

    let header = |text: &str| -> Line<'static> {
        Line::from(Span::styled(
            format!("  {}", text),
            Style::default().fg(BRASS).add_modifier(Modifier::BOLD),
        ))
    };
    let kv = |key: &str, val: &str| -> Line<'static> {
        Line::from(vec![
            Span::styled(format!("    {}: ", key), Style::default().fg(STONE)),
            Span::styled(val.to_string(), Style::default().fg(CREAM)),
        ])
    };
    let separator = || -> Line<'static> { Line::from("") };
    // Thin divider between settings sections.
    let section_div = || -> Line<'static> {
        Line::from(Span::styled(
            format!("  {}", "─".repeat(36)),
            Style::default().fg(STONE),
        ))
    };

    let mut lines: Vec<Line<'static>> = Vec::new();

    // ── GENERAL ──────────────────────────────────────────────────────────────
    lines.push(header("GENERAL"));
    lines.push(kv("Default Agent", &cfg.default_agent));
    lines.push(kv("Theme", &cfg.theme));
    lines.push(kv("Tick Rate", &format!("{}ms", cfg.tick_rate_ms)));
    lines.push(kv("Model", &state.model));
    lines.push(separator());
    lines.push(section_div());
    lines.push(separator());

    // ── AGENTS ───────────────────────────────────────────────────────────────
    lines.push(header("AGENTS"));
    if dash.available_agents.is_empty() {
        lines.push(Line::from(Span::styled(
            "    No agents detected.".to_string(),
            Style::default().fg(MUTED),
        )));
    } else {
        for agent in &dash.available_agents {
            let (indicator, fg) = if agent.available {
                ("●", SPROUT)
            } else {
                ("○", MUTED)
            };
            let binary = agent
                .binary_path
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "—".to_string());
            lines.push(Line::from(vec![
                Span::styled(format!("    {} ", indicator), Style::default().fg(fg)),
                Span::styled(
                    agent.name.clone(),
                    Style::default().fg(if agent.available { CREAM } else { MUTED }),
                ),
            ]));
            lines.push(kv("      Adapter", &agent.adapter));
            lines.push(kv("      Binary", &binary));
        }
    }
    lines.push(separator());
    lines.push(section_div());
    lines.push(separator());

    // ── KEYBINDS ─────────────────────────────────────────────────────────────
    lines.push(header("KEYBINDS"));
    let kb = &cfg.keybinds;
    lines.push(kv("Quit", &kb.quit));
    lines.push(kv("Submit", &kb.submit));
    lines.push(kv("Model Picker", &kb.model_picker));
    lines.push(kv("Help", &kb.help));
    lines.push(kv("Approve", &kb.approve));
    lines.push(kv("Deny", &kb.deny));
    lines.push(kv("New Session", &kb.new_session));
    lines.push(separator());
    lines.push(section_div());
    lines.push(separator());

    // ── PATHS ────────────────────────────────────────────────────────────────
    lines.push(header("PATHS"));
    let config_display = if state.config_path.is_empty() {
        "default".to_string()
    } else {
        state.config_path.clone()
    };
    lines.push(kv("Config", &config_display));
    lines.push(kv("Session DB", &cfg.db_path));
    lines.push(kv("CWD", &dash.path_snapshots.cwd));

    let potato_status = if dash.path_snapshots.potato_exists {
        "found"
    } else {
        "not found"
    };
    lines.push(kv(".potato/", potato_status));

    let openspec_status = if dash.path_snapshots.openspec_exists {
        "found"
    } else {
        "not found"
    };
    lines.push(kv("openspec/", openspec_status));

    let mcp_json_status = if dash.path_snapshots.mcp_json_exists {
        "found"
    } else {
        "not found"
    };
    lines.push(kv(".mcp.json", mcp_json_status));

    let agents_toml_status = if dash.path_snapshots.agents_toml_exists {
        format!("found ({} profiles)", state.agent_profiles.len())
    } else {
        "not found".to_string()
    };
    lines.push(kv("agents.toml", &agents_toml_status));
    lines.push(separator());
    lines.push(section_div());
    lines.push(separator());

    // ── MCP / COORDINATION ───────────────────────────────────────────────────
    lines.push(header("MCP / COORDINATION"));
    let socket_display = state
        .mcp_socket_path
        .as_ref()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "inactive".to_string());
    lines.push(kv("Socket", &socket_display));

    let iss_status = if state.inter_session_state.is_some() {
        "active"
    } else {
        "inactive"
    };
    lines.push(kv("Inter-Session", iss_status));

    // Live data from InterSessionState (the actual MCP runtime).
    let (pane_count, live_roles) = state
        .inter_session_state
        .as_ref()
        .and_then(|iss| iss.lock().ok())
        .map(|st| {
            let count = st.known_panes.len();
            let roles: Vec<String> = st
                .list_roles()
                .iter()
                .map(|(id, r)| format!("{} (pane {})", r.name, id))
                .collect();
            (count, roles)
        })
        .unwrap_or((0, Vec::new()));

    lines.push(kv("Registered Panes", &pane_count.to_string()));

    if live_roles.is_empty() {
        lines.push(kv("Active Roles", "none"));
    } else {
        lines.push(kv("Active Roles", &live_roles.join(", ")));
    }
    lines.push(separator());
    lines.push(section_div());
    lines.push(separator());

    // ── PERMISSIONS ──────────────────────────────────────────────────────────
    lines.push(header("PERMISSIONS"));
    lines.push(kv("Mode", "dangerously-skip-permissions (always on)"));

    lines
}

fn render_detail_settings(frame: &mut Frame, area: Rect, state: &AppState) {
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
        .title(Span::styled(" Settings ", Style::default().fg(TAN)))
        .borders(Borders::ALL)
        .border_style(border_style)
        .style(Style::default().bg(BG));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let all_lines = build_settings_lines(state);

    // Apply scroll from settings_scroll, clamped to content height.
    let total = all_lines.len() as u16;
    let max_scroll = total.saturating_sub(inner.height);
    let scroll = dash.settings_scroll.min(max_scroll);

    let content = Paragraph::new(all_lines).scroll((scroll, 0));
    frame.render_widget(content, inner);
}

// ── Footer ────────────────────────────────────────────────────────────────────

fn render_footer(frame: &mut Frame, area: Rect, state: &AppState) {
    let AppScreen::Dashboard(ref dash) = state.screen else {
        return;
    };

    let hints = match (DashboardMenuItem::ALL[dash.selected_menu], &dash.focus) {
        (DashboardMenuItem::RoastPotato, DashboardFocus::Menu) => {
            "↑↓ navigate  Tab details  Enter launch  Ctrl+Q quit"
        }
        (_, DashboardFocus::Menu) => "↑↓ navigate  Tab details  Enter select  Ctrl+Q quit",
        (DashboardMenuItem::Settings, DashboardFocus::Detail) => "↑↓ scroll  Tab menu  Esc back",
        (_, DashboardFocus::Detail) => "↑↓ navigate  Tab menu  Esc back  Enter select",
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
    use crate::app::state::{
        AgentInfo, DashboardFocus, DashboardMenuItem, DashboardState, RoleDefinition,
        SessionSummary,
    };
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

    // ── Settings panel tests ─────────────────────────────────────────────────

    fn make_settings_state() -> AppState {
        use crate::app::state::PathSnapshots;

        let mut state = AppState {
            config_path: "/home/user/.potato/config.toml".to_string(),
            model: "sonnet-4".to_string(),
            mcp_socket_path: Some(std::path::PathBuf::from("/tmp/potato.sock")),
            ..Default::default()
        };
        if let Some(dash) = state.dashboard_mut() {
            dash.available_agents = vec![
                AgentInfo {
                    name: "Claude Code".to_string(),
                    adapter: "claude".to_string(),
                    binary_path: Some(std::path::PathBuf::from("/usr/bin/claude")),
                    available: true,
                },
                AgentInfo {
                    name: "Codex".to_string(),
                    adapter: "codex".to_string(),
                    binary_path: None,
                    available: false,
                },
            ];
            dash.path_snapshots = PathSnapshots {
                cwd: "/home/user/projects/potato".to_string(),
                potato_exists: true,
                openspec_exists: true,
                mcp_json_exists: false,
                agents_toml_exists: false,
            };
        }
        state
    }

    /// Helper: collect all lines into a single string for substring matching.
    fn lines_to_string(lines: &[ratatui::text::Line<'_>]) -> String {
        lines
            .iter()
            .map(|l| {
                l.spans
                    .iter()
                    .map(|s| s.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn settings_lines_has_general_section() {
        let state = make_settings_state();
        let lines = super::build_settings_lines(&state);
        let text = lines_to_string(&lines);
        assert!(text.contains("GENERAL"), "missing GENERAL header");
        assert!(text.contains("claude"), "missing default_agent value");
        assert!(text.contains("earth"), "missing theme value");
        assert!(text.contains("250"), "missing tick_rate_ms value");
    }

    #[test]
    fn settings_lines_has_agents_section() {
        let state = make_settings_state();
        let lines = super::build_settings_lines(&state);
        let text = lines_to_string(&lines);
        assert!(text.contains("AGENTS"), "missing AGENTS header");
        assert!(text.contains("Claude Code"), "missing agent name");
        assert!(text.contains("Codex"), "missing second agent");
    }

    #[test]
    fn settings_lines_has_keybinds_section() {
        let state = make_settings_state();
        let lines = super::build_settings_lines(&state);
        let text = lines_to_string(&lines);
        assert!(text.contains("KEYBINDS"), "missing KEYBINDS header");
        assert!(text.contains("ctrl+q"), "missing quit keybind");
        assert!(text.contains("enter"), "missing submit keybind");
    }

    #[test]
    fn settings_lines_has_paths_section() {
        let state = make_settings_state();
        let lines = super::build_settings_lines(&state);
        let text = lines_to_string(&lines);
        assert!(text.contains("PATHS"), "missing PATHS header");
        assert!(
            text.contains("/home/user/.potato/config.toml"),
            "missing config path"
        );
        assert!(
            text.contains("/home/user/projects/potato"),
            "missing CWD from snapshot"
        );
        // .potato/ should be "found", .mcp.json should be "not found" per snapshot
        assert!(text.contains(".potato/: found"), "missing .potato/ status");
        assert!(
            text.contains(".mcp.json: not found"),
            "missing .mcp.json status"
        );
    }

    #[test]
    fn settings_lines_has_mcp_section() {
        let state = make_settings_state();
        let lines = super::build_settings_lines(&state);
        let text = lines_to_string(&lines);
        assert!(text.contains("MCP"), "missing MCP header");
        assert!(text.contains("/tmp/potato.sock"), "missing socket path");
        // No InterSessionState set up → panes 0, roles "none"
        assert!(text.contains("Registered Panes: 0"), "missing pane count");
        assert!(
            text.contains("Active Roles: none"),
            "missing roles when ISS absent"
        );
    }

    #[test]
    fn settings_lines_mcp_shows_live_roles() {
        use crate::mcp::state::{InterSessionState, PaneRole};
        use std::sync::{Arc, Mutex};

        let mut state = make_settings_state();
        let iss = Arc::new(Mutex::new(InterSessionState::default()));
        {
            let mut st = iss.lock().unwrap();
            st.register_pane(0);
            st.register_pane(1);
            st.set_role(
                0,
                PaneRole {
                    name: "Planner".to_string(),
                    description: String::new(),
                },
            );
            st.set_role(
                1,
                PaneRole {
                    name: "Worker".to_string(),
                    description: String::new(),
                },
            );
        }
        state.inter_session_state = Some(iss);

        let lines = super::build_settings_lines(&state);
        let text = lines_to_string(&lines);
        assert!(
            text.contains("Registered Panes: 2"),
            "should show 2 live panes"
        );
        assert!(text.contains("Planner"), "should show Planner role");
        assert!(text.contains("Worker"), "should show Worker role");
    }

    #[test]
    fn settings_lines_has_permissions_section() {
        let state = make_settings_state();
        let lines = super::build_settings_lines(&state);
        let text = lines_to_string(&lines);
        assert!(text.contains("PERMISSIONS"), "missing PERMISSIONS header");
    }

    #[test]
    fn settings_lines_section_count() {
        let state = make_settings_state();
        let lines = super::build_settings_lines(&state);
        let text = lines_to_string(&lines);
        // 6 sections: GENERAL, AGENTS, KEYBINDS, PATHS, MCP, PERMISSIONS
        let section_headers = [
            "GENERAL",
            "AGENTS",
            "KEYBINDS",
            "PATHS",
            "MCP",
            "PERMISSIONS",
        ];
        for header in &section_headers {
            assert!(text.contains(header), "missing section: {}", header);
        }
    }

    #[test]
    fn settings_scroll_clamps_to_content() {
        let mut state = make_settings_state();
        let lines = super::build_settings_lines(&state);
        let total = lines.len() as u16;
        // Scroll beyond content should clamp in render (we test the max here).
        let panel_height: u16 = 10;
        let max_scroll = total.saturating_sub(panel_height);
        // Setting scroll way beyond should be clamped.
        if let Some(dash) = state.dashboard_mut() {
            dash.settings_scroll = 999;
        }
        let dash = state.dashboard().unwrap();
        let clamped = dash.settings_scroll.min(max_scroll);
        assert!(clamped <= max_scroll);
        assert!(clamped < 999);
    }
}
