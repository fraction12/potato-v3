//! Quick Actions panel — context-sensitive discoverable actions in the sidebar.

use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};

use crate::app::state::{AppState, CockpitFocus};
use crate::ui::theme::{AMBER, BG, BRASS, CREAM, STONE, TAN};

/// A single quick action entry.
pub struct QuickAction {
    pub label: &'static str,
    pub keybind_hint: &'static str,
    pub kind: QuickActionKind,
}

/// What happens when a quick action is executed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuickActionKind {
    ToggleHelp,
    NewSession,
    RefreshGit,
    ExportSession,
    FocusTerminal,
    ClosePane,
    Approve,
    Deny,
}

/// Build the context-sensitive list of available actions.
pub fn actions_for_context(is_session: bool, has_approval: bool) -> Vec<QuickAction> {
    let mut actions = vec![
        QuickAction {
            label: "Toggle Help",
            keybind_hint: "F1",
            kind: QuickActionKind::ToggleHelp,
        },
        QuickAction {
            label: "New Session",
            keybind_hint: "/new",
            kind: QuickActionKind::NewSession,
        },
    ];

    if is_session {
        actions.push(QuickAction {
            label: "Refresh Git",
            keybind_hint: "F5",
            kind: QuickActionKind::RefreshGit,
        });
        actions.push(QuickAction {
            label: "Export Session",
            keybind_hint: "/export",
            kind: QuickActionKind::ExportSession,
        });
        actions.push(QuickAction {
            label: "Focus Terminal",
            keybind_hint: "F6",
            kind: QuickActionKind::FocusTerminal,
        });
        actions.push(QuickAction {
            label: "Close Pane",
            keybind_hint: "Ctrl+W",
            kind: QuickActionKind::ClosePane,
        });
    }

    if has_approval {
        actions.push(QuickAction {
            label: "Approve",
            keybind_hint: "y",
            kind: QuickActionKind::Approve,
        });
        actions.push(QuickAction {
            label: "Deny",
            keybind_hint: "n",
            kind: QuickActionKind::Deny,
        });
    }

    actions
}

/// Render the Quick Actions panel into the given area.
pub fn render(frame: &mut Frame, area: Rect, state: &AppState, focused: bool) {
    let title_color = if focused { AMBER } else { TAN };
    let border_fg = if focused { AMBER } else { BRASS };

    let has_approval = state
        .session()
        .and_then(|s| s.approval_pending.as_ref())
        .is_some();

    let is_session = matches!(state.screen, crate::app::state::AppScreen::Session(_));
    let actions = actions_for_context(is_session, has_approval);

    let selected = state.session().map(|s| s.selected_action).unwrap_or(0);

    let inner_w = area.width.saturating_sub(4) as usize;
    let mut lines: Vec<Line> = Vec::new();

    for (i, action) in actions.iter().enumerate() {
        let is_selected = focused && i == selected;
        let bg = if is_selected { AMBER } else { BG };
        let fg = if is_selected { BG } else { CREAM };
        let hint_fg = if is_selected { BG } else { STONE };

        // Right-align hint: "Label          Hint"
        let label_len = action.label.len();
        let hint_len = action.keybind_hint.len();
        let padding = inner_w.saturating_sub(label_len + hint_len + 1);
        let pad_str: String = " ".repeat(padding);

        lines.push(Line::from(vec![
            Span::styled(
                format!(" {}", action.label),
                Style::default().fg(fg).bg(bg).add_modifier(if is_selected {
                    Modifier::BOLD
                } else {
                    Modifier::empty()
                }),
            ),
            Span::styled(pad_str, Style::default().bg(bg)),
            Span::styled(
                format!("{} ", action.keybind_hint),
                Style::default().fg(hint_fg).bg(bg),
            ),
        ]));
    }

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_fg))
        .title(Span::styled(" Actions ", Style::default().fg(title_color)));

    frame.render_widget(
        Paragraph::new(lines)
            .block(block)
            .style(Style::default().bg(BG)),
        area,
    );
}

/// Number of actions in the current context (for bounds checking).
pub fn action_count(state: &AppState) -> usize {
    let has_approval = state
        .session()
        .and_then(|s| s.approval_pending.as_ref())
        .is_some();
    let is_session = matches!(state.screen, crate::app::state::AppScreen::Session(_));
    actions_for_context(is_session, has_approval).len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn always_shows_help_and_new_session() {
        let actions = actions_for_context(false, false);
        assert!(
            actions
                .iter()
                .any(|a| a.kind == QuickActionKind::ToggleHelp)
        );
        assert!(
            actions
                .iter()
                .any(|a| a.kind == QuickActionKind::NewSession)
        );
        assert_eq!(actions.len(), 2);
    }

    #[test]
    fn session_context_adds_session_actions() {
        let actions = actions_for_context(true, false);
        assert!(
            actions
                .iter()
                .any(|a| a.kind == QuickActionKind::RefreshGit)
        );
        assert!(
            actions
                .iter()
                .any(|a| a.kind == QuickActionKind::FocusTerminal)
        );
        assert!(actions.iter().any(|a| a.kind == QuickActionKind::ClosePane));
        assert!(
            actions
                .iter()
                .any(|a| a.kind == QuickActionKind::ExportSession)
        );
        assert_eq!(actions.len(), 6);
    }

    #[test]
    fn approval_pending_adds_approve_deny() {
        let actions = actions_for_context(true, true);
        assert!(actions.iter().any(|a| a.kind == QuickActionKind::Approve));
        assert!(actions.iter().any(|a| a.kind == QuickActionKind::Deny));
        assert_eq!(actions.len(), 8);
    }

    #[test]
    fn dashboard_with_approval_only_adds_approve_deny() {
        let actions = actions_for_context(false, true);
        assert_eq!(actions.len(), 4); // help + new + approve + deny
    }

    #[test]
    fn keybind_hints_are_nonempty() {
        for a in actions_for_context(true, true) {
            assert!(!a.keybind_hint.is_empty(), "{} has empty hint", a.label);
        }
    }
}
