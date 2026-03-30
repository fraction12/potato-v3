//! Input focus key handling — text editing and Enter broadcast.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::app::state::{AppScreen, AppState};

use super::KeyAction;

/// Handle a key event when Input focus is active.
pub fn handle(state: &mut AppState, key: &KeyEvent) -> KeyAction {
    // ── Text editing and Enter ───────────────────────────────────────
    if let AppScreen::Session(ref mut session) = state.screen {
        match key.code {
            KeyCode::Enter => {
                let text = std::mem::take(&mut session.input_buffer);
                session.input_cursor = 0;
                session.reset_terminal_scroll();

                if !text.is_empty() {
                    return KeyAction::Broadcast(text);
                }
                return KeyAction::Handled;
            }
            KeyCode::Backspace => {
                session.input_buffer.pop();
                if session.input_cursor > session.input_buffer.len() {
                    session.input_cursor = session.input_buffer.len();
                }
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
            KeyCode::Home => {
                session.input_cursor = 0;
                return KeyAction::Handled;
            }
            KeyCode::End => {
                session.input_cursor = session.input_buffer.len();
                return KeyAction::Handled;
            }
            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                session.input_buffer.push(c);
                session.input_cursor = session.input_buffer.len();
                return KeyAction::Handled;
            }
            _ => {}
        }
    }

    KeyAction::Unhandled
}
