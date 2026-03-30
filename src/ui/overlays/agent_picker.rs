//! Agent picker overlay — lists detected agents and lets the user launch one.
//!
//! Triggered by `/agent` command or Ctrl+A (configurable keybind).
//!
//! Layout:
//! ```
//! ┌──────────── Agent Picker ────────────┐
//! │  Name          Binary          Caps  │
//! │ ► Claude Code  /usr/bin/claude  S A  │
//! │   Codex        not found             │
//! │   OpenCode     /usr/local/…     S    │
//! │                                      │
//! │  Enter: launch  Esc: cancel          │
//! └──────────────────────────────────────┘
//! ```
//!
//! Capabilities abbreviations: S=structured, R=resumable, A=approval, T=tools.

use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph},
};

use crate::adapters::{
    AdapterCapabilities, AgentAdapter, claude::ClaudeAdapter, codex::CodexAdapter,
    generic::GenericAdapter,
};
use crate::app::state::AgentPickerState;

// ── Theme colors ──────────────────────────────────────────────────────────────

const BG: Color = Color::Rgb(28, 28, 32);
const BORDER: Color = Color::Rgb(80, 80, 100);
const HEADER: Color = Color::Rgb(160, 160, 200);
const SELECTED_BG: Color = Color::Rgb(45, 45, 65);
const AVAILABLE: Color = Color::Rgb(130, 200, 130);
const UNAVAILABLE: Color = Color::Rgb(120, 80, 80);
const CAP_COLOR: Color = Color::Rgb(180, 160, 100);
const HINT: Color = Color::Rgb(90, 90, 110);

// ── AgentRow ──────────────────────────────────────────────────────────────────

/// A single row in the agent picker.
#[derive(Debug, Clone)]
pub struct AgentRow {
    pub display_name: String,
    pub adapter_name: String,
    pub binary_display: String,
    pub available: bool,
    pub caps: AdapterCapabilities,
}

impl AgentRow {
    /// Abbreviated capabilities string.
    ///
    /// Characters: `S`=structured, `R`=resumable, `A`=approval, `T`=tools.
    /// Missing capabilities are shown as `.`.
    #[must_use]
    pub fn caps_str(&self) -> String {
        let mut s = String::with_capacity(4);
        s.push(if self.caps.structured_output {
            'S'
        } else {
            '.'
        });
        s.push(if self.caps.session_resumable {
            'R'
        } else {
            '.'
        });
        s.push(if self.caps.approval_intercept {
            'A'
        } else {
            '.'
        });
        s.push(if self.caps.tool_events { 'T' } else { '.' });
        s
    }
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Build the list of detectable agents for display in the picker.
///
/// Always includes Claude, Codex, and an OpenCode generic fallback.
pub fn build_agent_rows() -> Vec<AgentRow> {
    let claude = ClaudeAdapter;
    let codex = CodexAdapter;
    let opencode = GenericAdapter::new("opencode");

    vec![
        AgentRow {
            display_name: "Claude Code".to_string(),
            adapter_name: "claude".to_string(),
            binary_display: claude
                .detect()
                .and_then(|p| p.to_str().map(str::to_string))
                .unwrap_or_else(|| "not found".to_string()),
            available: claude.detect().is_some(),
            caps: claude.capabilities(),
        },
        AgentRow {
            display_name: "Codex".to_string(),
            adapter_name: "codex".to_string(),
            binary_display: codex
                .detect()
                .and_then(|p| p.to_str().map(str::to_string))
                .unwrap_or_else(|| "not found".to_string()),
            available: codex.detect().is_some(),
            caps: codex.capabilities(),
        },
        AgentRow {
            display_name: "OpenCode".to_string(),
            adapter_name: "opencode".to_string(),
            binary_display: opencode
                .detect()
                .and_then(|p| p.to_str().map(str::to_string))
                .unwrap_or_else(|| "not found".to_string()),
            available: opencode.detect().is_some(),
            caps: opencode.capabilities(),
        },
    ]
}

/// Render the agent picker overlay centred in `area`.
pub fn render_agent_picker(
    frame: &mut Frame,
    area: Rect,
    picker_state: &AgentPickerState,
    rows: &[AgentRow],
) {
    // Overlay dimensions: 60 wide, rows + 6 lines tall.
    let width = 62u16.min(area.width.saturating_sub(4));
    let height = (rows.len() as u16 + 6).min(area.height.saturating_sub(4));

    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;

    let overlay_area = Rect {
        x,
        y,
        width,
        height,
    };

    // Clear underlying content.
    frame.render_widget(Clear, overlay_area);

    // Outer block.
    let block = Block::default()
        .title(" Agent Picker ")
        .title_style(Style::default().fg(HEADER).add_modifier(Modifier::BOLD))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(BORDER))
        .style(Style::default().bg(BG));

    let inner = block.inner(overlay_area);
    frame.render_widget(block, overlay_area);

    // Split inner into: header row | rows... | footer hint
    let constraints: Vec<Constraint> = std::iter::once(Constraint::Length(1))
        .chain(rows.iter().map(|_| Constraint::Length(1)))
        .chain(std::iter::once(Constraint::Length(1)))
        .chain(std::iter::once(Constraint::Length(1)))
        .collect();

    let chunks = Layout::vertical(constraints).split(inner);

    // ── Column widths ─────────────────────────────────────────────────────────
    let name_w = 14usize;
    let binary_w = (inner.width as usize).saturating_sub(name_w + 7); // 7 = caps(4) + spacing
    let binary_w = binary_w.max(10);

    // ── Header ────────────────────────────────────────────────────────────────
    if !chunks.is_empty() {
        let header = Line::from(vec![
            Span::raw(format!("  {:<name_w$}", "Name")),
            Span::raw(format!("{:<binary_w$}", "Binary")),
            Span::styled("Caps", Style::default().fg(CAP_COLOR)),
        ]);
        frame.render_widget(
            Paragraph::new(header).style(Style::default().fg(HEADER)),
            chunks[0],
        );
    }

    // ── Agent rows ────────────────────────────────────────────────────────────
    for (i, row) in rows.iter().enumerate() {
        let chunk_idx = i + 1;
        if chunk_idx >= chunks.len() {
            break;
        }

        let is_selected = i == picker_state.selected;
        let cursor = if is_selected { "►" } else { " " };

        // Truncate binary path to fit.
        let bin_display = if row.binary_display.len() > binary_w {
            let start = row.binary_display.len() - binary_w + 1;
            format!("…{}", &row.binary_display[start..])
        } else {
            row.binary_display.clone()
        };

        let name_color = if row.available {
            AVAILABLE
        } else {
            UNAVAILABLE
        };
        let bin_color = if row.available {
            AVAILABLE
        } else {
            UNAVAILABLE
        };

        let row_style = if is_selected {
            Style::default().bg(SELECTED_BG)
        } else {
            Style::default().bg(BG)
        };

        let line = Line::from(vec![
            Span::raw(format!("{cursor} ")),
            Span::styled(
                format!(
                    "{:<name_w$}",
                    &row.display_name[..row.display_name.len().min(name_w)]
                ),
                Style::default().fg(name_color),
            ),
            Span::styled(
                format!("{:<binary_w$}", bin_display),
                Style::default().fg(bin_color),
            ),
            Span::styled(row.caps_str(), Style::default().fg(CAP_COLOR)),
        ]);
        frame.render_widget(Paragraph::new(line).style(row_style), chunks[chunk_idx]);
    }

    // ── Separator ─────────────────────────────────────────────────────────────
    let sep_idx = rows.len() + 1;
    if sep_idx < chunks.len() {
        frame.render_widget(
            Paragraph::new(Line::from(vec![Span::styled(
                "─".repeat(inner.width as usize),
                Style::default().fg(BORDER),
            )])),
            chunks[sep_idx],
        );
    }

    // ── Footer hint ───────────────────────────────────────────────────────────
    let hint_idx = rows.len() + 2;
    if hint_idx < chunks.len() {
        let hint = Line::from(vec![
            Span::styled("↑↓", Style::default().fg(HEADER)),
            Span::styled(" navigate  ", Style::default().fg(HINT)),
            Span::styled("Enter", Style::default().fg(HEADER)),
            Span::styled(" launch  ", Style::default().fg(HINT)),
            Span::styled("Esc", Style::default().fg(HEADER)),
            Span::styled(" cancel", Style::default().fg(HINT)),
        ]);
        frame.render_widget(Paragraph::new(hint), chunks[hint_idx]);
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_agent_rows_has_three_entries() {
        let rows = build_agent_rows();
        assert_eq!(rows.len(), 3);
    }

    #[test]
    fn build_agent_rows_names() {
        let rows = build_agent_rows();
        let names: Vec<&str> = rows.iter().map(|r| r.display_name.as_str()).collect();
        assert!(names.contains(&"Claude Code"));
        assert!(names.contains(&"Codex"));
        assert!(names.contains(&"OpenCode"));
    }

    #[test]
    fn build_agent_rows_adapter_names() {
        let rows = build_agent_rows();
        let adapters: Vec<&str> = rows.iter().map(|r| r.adapter_name.as_str()).collect();
        assert!(adapters.contains(&"claude"));
        assert!(adapters.contains(&"codex"));
        assert!(adapters.contains(&"opencode"));
    }

    #[test]
    fn caps_str_all_false_gives_dots() {
        let row = AgentRow {
            display_name: "X".into(),
            adapter_name: "x".into(),
            binary_display: "not found".into(),
            available: false,
            caps: AdapterCapabilities {
                structured_output: false,
                session_resumable: false,
                approval_intercept: false,
                tool_events: false,
            },
        };
        assert_eq!(row.caps_str(), "....");
    }

    #[test]
    fn caps_str_all_true_gives_srat() {
        let row = AgentRow {
            display_name: "X".into(),
            adapter_name: "x".into(),
            binary_display: "/bin/x".into(),
            available: true,
            caps: AdapterCapabilities {
                structured_output: true,
                session_resumable: true,
                approval_intercept: true,
                tool_events: true,
            },
        };
        assert_eq!(row.caps_str(), "SRAT");
    }

    #[test]
    fn caps_str_claude_pattern() {
        // Claude: structured=true, resumable=true, approval=true, tools=true → SRAT
        let row = build_agent_rows()
            .into_iter()
            .find(|r| r.adapter_name == "claude")
            .unwrap();
        assert_eq!(row.caps_str(), "SRAT");
    }

    #[test]
    fn caps_str_codex_pattern() {
        // Codex: structured=true, resumable=true, approval=false, tools=true → SR.T
        let row = build_agent_rows()
            .into_iter()
            .find(|r| r.adapter_name == "codex")
            .unwrap();
        assert_eq!(row.caps_str(), "SR.T");
    }

    #[test]
    fn caps_str_generic_pattern() {
        // OpenCode (generic): all false → ....
        let row = build_agent_rows()
            .into_iter()
            .find(|r| r.adapter_name == "opencode")
            .unwrap();
        assert_eq!(row.caps_str(), "....");
    }

    #[test]
    fn agent_row_binary_display_not_found_when_unavailable() {
        // For agents not installed, binary_display should be "not found".
        let rows = build_agent_rows();
        for row in &rows {
            if !row.available {
                assert_eq!(
                    row.binary_display, "not found",
                    "unavailable agent {} should show 'not found'",
                    row.display_name
                );
            }
        }
    }

    #[test]
    fn agent_row_available_means_binary_is_a_path() {
        let rows = build_agent_rows();
        for row in &rows {
            if row.available {
                assert!(
                    row.binary_display.starts_with('/'),
                    "available agent {} should have absolute path, got {}",
                    row.display_name,
                    row.binary_display
                );
            }
        }
    }
}
