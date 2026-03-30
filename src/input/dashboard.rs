//! Dashboard screen key handling.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::app::state::{
    AppScreen, AppState, DashboardFocus, DashboardInput, DashboardMenuItem,
};

use super::KeyAction;

/// Handle a key event on the Dashboard screen.
pub fn handle(state: &mut AppState, key: &KeyEvent) -> KeyAction {
    // ── Ctrl+\ — quit from any dashboard context ─────────────────────
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('\\') {
        return KeyAction::Quit;
    }

    let AppScreen::Dashboard(ref mut dash) = state.screen else {
        return KeyAction::Unhandled;
    };

    // ── Inline input submode (role name/prompt entry) ────────────────
    if dash.input != DashboardInput::None {
        return handle_inline_input(dash, key);
    }

    // ── Enter dispatch ───────────────────────────────────────────────
    if key.code == KeyCode::Enter {
        let menu_item = DashboardMenuItem::ALL[dash.selected_menu];
        match (menu_item, &dash.focus) {
            (DashboardMenuItem::RoastPotato, DashboardFocus::Menu) => {
                return KeyAction::SpawnDashboard;
            }
            (DashboardMenuItem::RoastPotato, DashboardFocus::Detail) => {
                if dash.selected_detail < dash.recent_sessions.len() {
                    let id = dash.recent_sessions[dash.selected_detail].session_id.clone();
                    return KeyAction::ResumeSession(id);
                }
            }
            _ => {
                if dash.focus == DashboardFocus::Menu {
                    dash.focus = DashboardFocus::Detail;
                    dash.selected_detail = 0;
                }
                return KeyAction::Handled;
            }
        }
    }

    // ── F2/F3 — role add/delete (DefineRoles detail only) ────────────
    let menu_item = DashboardMenuItem::ALL[dash.selected_menu];
    if menu_item == DashboardMenuItem::DefineRoles && dash.focus == DashboardFocus::Detail {
        match key.code {
            KeyCode::F(2) => {
                dash.input = DashboardInput::RoleName(String::new());
                return KeyAction::Handled;
            }
            KeyCode::F(3) => {
                if !dash.roles.is_empty() && dash.selected_detail < dash.roles.len() {
                    dash.roles.remove(dash.selected_detail);
                    if dash.selected_detail > 0 && dash.selected_detail >= dash.roles.len() {
                        dash.selected_detail = dash.roles.len().saturating_sub(1);
                    }
                    if let Ok(cwd) = std::env::current_dir() {
                        if let Err(e) = crate::roles::save_roles(&cwd, &dash.roles) {
                            tracing::error!("Failed to save roles: {e}");
                        }
                    }
                }
                return KeyAction::Handled;
            }
            _ => {}
        }
    }

    // ── Tab — toggle Menu ↔ Detail ───────────────────────────────────
    if key.code == KeyCode::Tab {
        dash.focus = match dash.focus {
            DashboardFocus::Menu => DashboardFocus::Detail,
            DashboardFocus::Detail => DashboardFocus::Menu,
        };
        return KeyAction::Handled;
    }

    // ── Arrow navigation ─────────────────────────────────────────────
    match key.code {
        KeyCode::Up => {
            match dash.focus {
                DashboardFocus::Menu if dash.selected_menu > 0 => {
                    dash.selected_menu -= 1;
                }
                DashboardFocus::Detail => {
                    let item = DashboardMenuItem::ALL[dash.selected_menu];
                    if item == DashboardMenuItem::Settings {
                        dash.settings_scroll = dash.settings_scroll.saturating_sub(1);
                    } else if dash.selected_detail > 0 {
                        dash.selected_detail -= 1;
                    }
                }
                _ => {}
            }
            return KeyAction::Handled;
        }
        KeyCode::Down => {
            match dash.focus {
                DashboardFocus::Menu => {
                    let max = DashboardMenuItem::ALL.len().saturating_sub(1);
                    if dash.selected_menu < max {
                        dash.selected_menu += 1;
                    }
                }
                DashboardFocus::Detail => {
                    let item = DashboardMenuItem::ALL[dash.selected_menu];
                    if item == DashboardMenuItem::Settings {
                        dash.settings_scroll = dash.settings_scroll.saturating_add(1);
                    } else {
                        let max = match item {
                            DashboardMenuItem::DefineRoles => dash.roles.len().saturating_sub(1),
                            DashboardMenuItem::RoastPotato => {
                                dash.recent_sessions.len().saturating_sub(1)
                            }
                            _ => 0,
                        };
                        if dash.selected_detail < max {
                            dash.selected_detail += 1;
                        }
                    }
                }
            }
            return KeyAction::Handled;
        }
        _ => {}
    }

    // ── Esc ──────────────────────────────────────────────────────────
    if key.code == KeyCode::Esc {
        if dash.focus == DashboardFocus::Detail {
            dash.focus = DashboardFocus::Menu;
        }
        // Esc on Menu is no-op (Ctrl+\ to quit).
        return KeyAction::Handled;
    }

    KeyAction::Unhandled
}

/// Handle keys during inline role input (name or prompt entry).
fn handle_inline_input(
    dash: &mut crate::app::state::DashboardState,
    key: &KeyEvent,
) -> KeyAction {
    match &mut dash.input {
        DashboardInput::RoleName(buf) => match key.code {
            KeyCode::Enter => {
                let name = buf.trim().to_string();
                if name.is_empty() {
                    dash.input = DashboardInput::None;
                } else {
                    dash.input = DashboardInput::RolePrompt { name, prompt: String::new() };
                }
            }
            KeyCode::Esc => dash.input = DashboardInput::None,
            KeyCode::Backspace => { buf.pop(); }
            KeyCode::Char(c) => buf.push(c),
            _ => {}
        },
        DashboardInput::RolePrompt { name, prompt } => match key.code {
            KeyCode::Enter => {
                let prompt_text = prompt.trim().to_string();
                if !prompt_text.is_empty() {
                    let role = crate::app::state::RoleDefinition {
                        name: name.clone(),
                        prompt: prompt_text,
                    };
                    dash.roles.push(role);
                    dash.selected_detail = dash.roles.len().saturating_sub(1);
                    if let Ok(cwd) = std::env::current_dir() {
                        if let Err(e) = crate::roles::save_roles(&cwd, &dash.roles) {
                            tracing::error!("Failed to save roles: {e}");
                        }
                    }
                }
                dash.input = DashboardInput::None;
            }
            KeyCode::Esc => dash.input = DashboardInput::None,
            KeyCode::Backspace => { prompt.pop(); }
            KeyCode::Char(c) => prompt.push(c),
            _ => {}
        },
        DashboardInput::None => unreachable!(), // guarded by caller
    }
    KeyAction::Handled
}
