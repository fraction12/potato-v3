//! Agents, Git, and Sidebar focus key handling.

use crossterm::event::{KeyCode, KeyEvent};

use crate::app::state::{AppScreen, AppState, CockpitFocus};

use super::KeyAction;

/// Handle a key event for Agents, Git, Tools, or Sidebar focus.
pub fn handle(state: &mut AppState, key: &KeyEvent, focus: CockpitFocus) -> KeyAction {
    match focus {
        CockpitFocus::Agents => handle_agents(state, key),
        CockpitFocus::Git => handle_git(state, key),
        CockpitFocus::Tools => handle_tools(state, key),
        CockpitFocus::Sidebar => handle_sidebar(state, key),
        _ => KeyAction::Unhandled,
    }
}

fn handle_agents(state: &mut AppState, key: &KeyEvent) -> KeyAction {
    if let AppScreen::Session(ref mut session) = state.screen {
        let agent_count = if state.agent_profiles.is_empty() {
            crate::ui::overlays::agent_picker::build_agent_rows().len()
        } else {
            state.agent_profiles.len()
        };
        let max_idx = agent_count.saturating_sub(1);
        match key.code {
            KeyCode::Up => {
                if session.selected_agent > 0 {
                    session.selected_agent -= 1;
                }
                return KeyAction::Handled;
            }
            KeyCode::Down => {
                if session.selected_agent < max_idx {
                    session.selected_agent += 1;
                }
                return KeyAction::Handled;
            }
            KeyCode::Home => {
                session.selected_agent = 0;
                return KeyAction::Handled;
            }
            KeyCode::End => {
                session.selected_agent = max_idx;
                return KeyAction::Handled;
            }
            KeyCode::Enter => {
                return KeyAction::SpawnAgent;
            }
            _ => {}
        }
    }
    KeyAction::Unhandled
}

fn handle_git(state: &mut AppState, key: &KeyEvent) -> KeyAction {
    if let AppScreen::Session(ref mut session) = state.screen {
        let handled = match key.code {
            KeyCode::Up => {
                session.git_scroll = session.git_scroll.saturating_sub(1);
                true
            }
            KeyCode::Down => {
                session.git_scroll = session.git_scroll.saturating_add(1);
                true
            }
            KeyCode::Home => {
                session.git_scroll = 0;
                true
            }
            KeyCode::End => {
                session.git_scroll = usize::MAX;
                true
            }
            KeyCode::PageUp => {
                session.git_scroll = session.git_scroll.saturating_sub(10);
                true
            }
            KeyCode::PageDown => {
                session.git_scroll = session.git_scroll.saturating_add(10);
                true
            }
            _ => false,
        };
        if handled {
            return KeyAction::Handled;
        }
    }
    KeyAction::Unhandled
}

fn handle_tools(state: &mut AppState, key: &KeyEvent) -> KeyAction {
    if let AppScreen::Session(ref mut session) = state.screen {
        let handled = match key.code {
            KeyCode::Up => {
                session.tools_scroll = session.tools_scroll.saturating_sub(1);
                true
            }
            KeyCode::Down => {
                session.tools_scroll = session.tools_scroll.saturating_add(1);
                true
            }
            KeyCode::Home => {
                session.tools_scroll = 0;
                true
            }
            KeyCode::End => {
                session.tools_scroll = usize::MAX;
                true
            }
            KeyCode::PageUp => {
                session.tools_scroll = session.tools_scroll.saturating_sub(10);
                true
            }
            KeyCode::PageDown => {
                session.tools_scroll = session.tools_scroll.saturating_add(10);
                true
            }
            _ => false,
        };
        if handled {
            return KeyAction::Handled;
        }
    }
    KeyAction::Unhandled
}

fn handle_sidebar(state: &mut AppState, key: &KeyEvent) -> KeyAction {
    use crate::ui::panels::quick_actions::{self, QuickActionKind};

    let max_idx = quick_actions::action_count(state).saturating_sub(1);

    match key.code {
        KeyCode::Up => {
            if let AppScreen::Session(ref mut session) = state.screen {
                if session.selected_action > 0 {
                    session.selected_action -= 1;
                }
            }
            return KeyAction::Handled;
        }
        KeyCode::Down => {
            if let AppScreen::Session(ref mut session) = state.screen {
                if session.selected_action < max_idx {
                    session.selected_action += 1;
                }
            }
            return KeyAction::Handled;
        }
        KeyCode::Enter => {
            let selected = state.session().map(|s| s.selected_action).unwrap_or(0);
            let has_approval = state
                .session()
                .and_then(|s| s.approval_pending.as_ref())
                .is_some();
            let actions = quick_actions::actions_for_context(true, has_approval);
            if let Some(action) = actions.get(selected) {
                return execute_quick_action(state, action.kind);
            }
        }
        _ => {}
    }
    KeyAction::Unhandled
}

/// Map a QuickActionKind to the corresponding KeyAction.
fn execute_quick_action(
    state: &mut AppState,
    kind: crate::ui::panels::quick_actions::QuickActionKind,
) -> KeyAction {
    use crate::ui::panels::quick_actions::QuickActionKind;

    match kind {
        QuickActionKind::ToggleHelp => {
            if let AppScreen::Session(ref mut session) = state.screen {
                if session.overlay == Some(crate::app::state::Overlay::Help) {
                    session.overlay = None;
                } else {
                    session.overlay = Some(crate::app::state::Overlay::Help);
                }
            }
            KeyAction::Handled
        }
        QuickActionKind::NewSession | QuickActionKind::ExportSession => KeyAction::SpawnAgent,
        QuickActionKind::RefreshGit => {
            state.git_snapshot = crate::git::GitSnapshot::refresh();
            state.git_refresh_ticks = 0;
            if let AppScreen::Session(ref mut session) = state.screen {
                session.git_scroll = 0;
            }
            KeyAction::Handled
        }
        QuickActionKind::FocusTerminal => {
            if let AppScreen::Session(ref mut session) = state.screen {
                session.cockpit_focus = CockpitFocus::Terminal;
            }
            KeyAction::Handled
        }
        QuickActionKind::ClosePane => KeyAction::ClosePane,
        QuickActionKind::Approve | QuickActionKind::Deny => {
            // Approval handling is done through PTY — not via quick actions directly.
            KeyAction::Handled
        }
    }
}
