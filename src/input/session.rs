//! Session screen key handling — global, overlay, esc, tab cycle.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::app::state::{AppScreen, AppState, CockpitFocus, Overlay};

use super::KeyAction;

/// Handle a key event on the Session screen.
pub fn handle(state: &mut AppState, key: &KeyEvent) -> KeyAction {
    // ── Global quit — Ctrl+\ always quits ────────────────────────────
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('\\') {
        return KeyAction::Quit;
    }

    let current_focus = state
        .session()
        .map(|s| s.cockpit_focus)
        .unwrap_or(CockpitFocus::Input);

    // ── F1 — toggle Help overlay ─────────────────────────────────────
    if key.code == KeyCode::F(1) && current_focus != CockpitFocus::Terminal {
        if let AppScreen::Session(ref mut session) = state.screen {
            if session.overlay == Some(Overlay::Help) {
                session.overlay = None;
            } else {
                session.overlay = Some(Overlay::Help);
            }
        }
        return KeyAction::Handled;
    }

    // ── F5 — refresh git, OpenSpec tasks, agent status ───────────────
    if key.code == KeyCode::F(5) && current_focus != CockpitFocus::Terminal {
        state.git_snapshot = crate::git::GitSnapshot::capture();
        state.git_refresh_ticks = 0;
        if let AppScreen::Session(ref mut session) = state.screen {
            session.git_scroll = 0;
        }
        return KeyAction::Handled;
    }

    // ── F6 — jump directly to Terminal focus ─────────────────────────
    if key.code == KeyCode::F(6) && current_focus != CockpitFocus::Terminal {
        if let AppScreen::Session(ref mut session) = state.screen {
            session.cockpit_focus = CockpitFocus::Terminal;
        }
        return KeyAction::Handled;
    }

    // ── Ctrl+W — close active pane (not from terminal focus) ─────────
    if key.modifiers.contains(KeyModifiers::CONTROL)
        && key.code == KeyCode::Char('w')
        && current_focus != CockpitFocus::Terminal
    {
        return KeyAction::ClosePane;
    }

    // ── Tab / Shift+Tab — cycle focus ring ───────────────────────────
    if key.code == KeyCode::Tab {
        let forward = !key.modifiers.contains(KeyModifiers::SHIFT);

        // In terminal focus, Shift+Tab passes through to PTY.
        if current_focus == CockpitFocus::Terminal && !forward {
            // Fall through to terminal PTY passthrough below.
        } else {
            let n_panes = state.panes.len();

            if n_panes > 1 && current_focus == CockpitFocus::Terminal {
                let active = state.panes.active_index();
                if active + 1 < n_panes {
                    state.panes.focus_next();
                    return KeyAction::Handled;
                }
            }

            if n_panes > 1 {
                if forward && current_focus == CockpitFocus::Input {
                    state.panes.focus(0);
                } else if !forward && current_focus == CockpitFocus::Sidebar {
                    state.panes.focus(n_panes - 1);
                }
            }

            if let AppScreen::Session(ref mut session) = state.screen {
                session.cockpit_focus = if forward {
                    session.cockpit_focus.next()
                } else {
                    session.cockpit_focus.prev()
                };
            }
            return KeyAction::Handled;
        }
    }

    // ── Overlay active — dispatch key to overlay ─────────────────────
    if let Some(action) = handle_overlay(state, key) {
        return action;
    }

    // ── Esc — context-sensitive ──────────────────────────────────────
    if key.code == KeyCode::Esc && current_focus != CockpitFocus::Terminal {
        match current_focus {
            CockpitFocus::Input => {
                if let AppScreen::Session(ref mut session) = state.screen {
                    if !session.input_buffer.is_empty() {
                        session.input_buffer.clear();
                    }
                }
                return KeyAction::Handled;
            }
            CockpitFocus::Terminal => unreachable!(),
            _ => {
                if let AppScreen::Session(ref mut session) = state.screen {
                    session.cockpit_focus = CockpitFocus::Input;
                }
                return KeyAction::Handled;
            }
        }
    }

    // ── Dispatch by focus ────────────────────────────────────────────
    match current_focus {
        CockpitFocus::Terminal => super::terminal::handle(state, key),
        CockpitFocus::Input => super::text_input::handle(state, key),
        CockpitFocus::Agents | CockpitFocus::Git | CockpitFocus::Sidebar => {
            super::panels::handle(state, key, current_focus)
        }
    }
}

/// Handle keys when an overlay is active. Returns `Some(action)` if consumed.
fn handle_overlay(state: &mut AppState, key: &KeyEvent) -> Option<KeyAction> {
    let overlay_kind = state.session().and_then(|s| s.overlay.clone())?;

    match overlay_kind {
        Overlay::AgentPicker => {
            match key.code {
                KeyCode::Esc => {
                    if let AppScreen::Session(ref mut session) = state.screen {
                        session.overlay = None;
                    }
                }
                KeyCode::Up => {
                    if let AppScreen::Session(ref mut session) = state.screen {
                        if session.agent_picker.selected > 0 {
                            session.agent_picker.selected -= 1;
                        }
                    }
                }
                KeyCode::Down => {
                    const MAX_AGENTS: usize = 2;
                    if let AppScreen::Session(ref mut session) = state.screen {
                        if session.agent_picker.selected < MAX_AGENTS {
                            session.agent_picker.selected += 1;
                        }
                    }
                }
                KeyCode::Enter => {
                    if let AppScreen::Session(ref mut session) = state.screen {
                        session.overlay = None;
                    }
                    return Some(KeyAction::SpawnAgent);
                }
                _ => {}
            }
            Some(KeyAction::Handled)
        }
        _ => {
            // Esc dismisses all other overlays; all keys are consumed.
            if key.code == KeyCode::Esc {
                if let AppScreen::Session(ref mut session) = state.screen {
                    session.overlay = None;
                }
            }
            Some(KeyAction::Handled)
        }
    }
}
