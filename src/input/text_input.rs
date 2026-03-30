//! Input focus key handling — text editing, slash commands, Enter broadcast.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::app::state::{AppScreen, AppState, Overlay};
use crate::commands::registry;

use super::KeyAction;

/// Handle a key event when Input focus is active.
pub fn handle(state: &mut AppState, key: &KeyEvent) -> KeyAction {
    // ── Autocomplete navigation (slash command mode) ─────────────────
    let in_command_mode = state
        .session()
        .map(|s| s.input_buffer.starts_with('/'))
        .unwrap_or(false);

    if in_command_mode {
        match key.code {
            KeyCode::Up => {
                if let AppScreen::Session(ref mut session) = state.screen {
                    let prefix = &session.input_buffer[1..];
                    let count = registry::completions(prefix).len();
                    if count > 0 {
                        if session.command_selected == 0 {
                            session.command_selected = count - 1;
                        } else {
                            session.command_selected -= 1;
                        }
                    }
                }
                return KeyAction::Handled;
            }
            KeyCode::Down => {
                if let AppScreen::Session(ref mut session) = state.screen {
                    let prefix = &session.input_buffer[1..];
                    let count = registry::completions(prefix).len();
                    if count > 0 {
                        session.command_selected = (session.command_selected + 1) % count;
                    }
                }
                return KeyAction::Handled;
            }
            KeyCode::Tab => {
                if let AppScreen::Session(ref mut session) = state.screen {
                    let prefix = session.input_buffer[1..].to_string();
                    let completions = registry::completions(&prefix);
                    let idx = session
                        .command_selected
                        .min(completions.len().saturating_sub(1));
                    if let Some(cmd) = completions.get(idx) {
                        session.input_buffer = format!("/{}", cmd.name);
                        session.input_cursor = session.input_buffer.len();
                        session.command_selected = 0;
                    }
                }
                return KeyAction::Handled;
            }
            _ => {}
        }
    }

    // ── Text editing and Enter ───────────────────────────────────────
    if let AppScreen::Session(ref mut session) = state.screen {
        match key.code {
            KeyCode::Enter => {
                let text = std::mem::take(&mut session.input_buffer);
                session.input_cursor = 0;
                session.command_selected = 0;
                session.reset_terminal_scroll();

                if !text.is_empty() {
                    if text.starts_with('/') {
                        return dispatch_slash_command(state, &text);
                    } else {
                        return KeyAction::Broadcast(text);
                    }
                }
                return KeyAction::Handled;
            }
            KeyCode::Backspace => {
                session.input_buffer.pop();
                if session.input_cursor > session.input_buffer.len() {
                    session.input_cursor = session.input_buffer.len();
                }
                session.command_selected = 0;
                return KeyAction::Handled;
            }
            KeyCode::Left if session.input_cursor > 0 => {
                session.input_cursor -= 1;
                return KeyAction::Handled;
            }
            KeyCode::Right if session.input_cursor < session.input_buffer.len() => {
                session.input_cursor += 1;
                return KeyAction::Handled;
            }
            KeyCode::Home => { session.input_cursor = 0; return KeyAction::Handled; }
            KeyCode::End => { session.input_cursor = session.input_buffer.len(); return KeyAction::Handled; }
            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                session.input_buffer.push(c);
                session.input_cursor = session.input_buffer.len();
                session.command_selected = 0;
                return KeyAction::Handled;
            }
            _ => {}
        }
    }

    KeyAction::Unhandled
}

/// Dispatch a slash command and return the appropriate action.
fn dispatch_slash_command(state: &mut AppState, text: &str) -> KeyAction {
    use registry::{CommandResult, OverlayKind};

    match registry::parse_command(text) {
        CommandResult::ShowOverlay(OverlayKind::Help) => {
            if let AppScreen::Session(ref mut session) = state.screen {
                session.overlay = Some(Overlay::Help);
            }
        }
        CommandResult::ShowOverlay(OverlayKind::Sessions) => {
            if let AppScreen::Session(ref mut session) = state.screen {
                session.overlay = Some(Overlay::Sessions);
            }
        }
        CommandResult::ShowOverlay(OverlayKind::AgentPicker) => {
            if let AppScreen::Session(ref mut session) = state.screen {
                session.overlay = Some(Overlay::AgentPicker);
            }
        }
        CommandResult::NewSession { .. } => {
            return KeyAction::SpawnAgent;
        }
        CommandResult::SetRole { name, description } => {
            handle_set_role(state, name, description);
        }
        CommandResult::Handled => {
            tracing::info!("Slash command handled: {}", text);
        }
        CommandResult::Unknown(cmd) => {
            state.set_error(
                format!("Unknown command: /{cmd}  (type /help for commands)"),
                80,
            );
        }
        CommandResult::PassThrough(_) => {
            // Should not happen since we checked starts_with('/').
        }
    }
    KeyAction::Handled
}

/// Handle /role command — set role on active pane and notify partners.
fn handle_set_role(state: &mut AppState, name: String, description: Option<String>) {
    let active_pane_id = state.panes.active_pane().map(|p| p.id);
    if let Some(pane) = state.panes.active_pane_mut() {
        pane.role_name = Some(name.clone());
        pane.role_description = description.clone();
        tracing::info!("Pane {} role set to '{}': {:?}", pane.id, name, description);
    }
    // Update InterSessionState so MCP tools reflect the role.
    if let Some(ref inter) = state.inter_session_state {
        if let Ok(mut is) = inter.lock() {
            if let Some(pid) = active_pane_id {
                is.set_role(
                    pid,
                    crate::mcp::state::PaneRole {
                        name: name.clone(),
                        description: description.clone().unwrap_or_default(),
                    },
                );
            }
        }
    }
    // Inject notification into ALL other panes.
    if let Some(pid) = active_pane_id {
        let n_panes = state.panes.len();
        let desc_part = description
            .as_deref()
            .filter(|d| !d.is_empty())
            .map(|d| format!(" — {d}"))
            .unwrap_or_default();
        let content = format!("🏷️ Partner Pane {pid} has set their role to: {name}{desc_part}");
        let notification =
            crate::mcp::injection::format_notification(pid, Some(&name), &content);
        for target in 0..n_panes {
            if state.panes.get(target).map(|p| p.id) != Some(pid) {
                if let Err(e) =
                    crate::mcp::injection::inject_into_pane(&mut state.panes, target, &notification)
                {
                    tracing::warn!("role inject to pane {target}: {e}");
                }
            }
        }
    }
}
