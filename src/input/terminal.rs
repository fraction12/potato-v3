//! Terminal focus key handling — viewport scroll + PTY passthrough.

use crossterm::event::{KeyCode, KeyEvent};

use crate::app::state::AppState;
use crate::pty::key_event_to_bytes;

use super::KeyAction;

/// Handle a key event when Terminal focus is active.
pub fn handle(state: &mut AppState, key: &KeyEvent) -> KeyAction {
    // ── Viewport scroll (PageUp/Down/Home/End) ──────────────────────
    let has_pane = !state.panes.is_empty();
    let scroll_handled = if has_pane {
        if let Some(pane) = state.panes.active_pane_mut() {
            match key.code {
                KeyCode::PageUp => {
                    pane.session.scroll_terminal_up(10);
                    true
                }
                KeyCode::PageDown => {
                    pane.session.scroll_terminal_down(10);
                    true
                }
                KeyCode::Home => {
                    pane.session.scroll_terminal_up(10_000);
                    true
                }
                KeyCode::End => {
                    pane.session.reset_terminal_scroll();
                    true
                }
                _ => false,
            }
        } else {
            false
        }
    } else if let Some(session) = state.session_mut() {
        match key.code {
            KeyCode::PageUp => {
                session.scroll_terminal_up(10);
                true
            }
            KeyCode::PageDown => {
                session.scroll_terminal_down(10);
                true
            }
            KeyCode::Home => {
                session.scroll_terminal_up(10_000);
                true
            }
            KeyCode::End => {
                session.reset_terminal_scroll();
                true
            }
            _ => false,
        }
    } else {
        false
    };
    if scroll_handled {
        return KeyAction::Handled;
    }

    // ── PTY passthrough — all other keys go to agent ─────────────────
    let raw_bytes = key_event_to_bytes(*key);
    if !raw_bytes.is_empty() {
        if let Some(pane) = state.panes.active_pane_mut() {
            if let Some(ref mut pty) = pane.pty {
                if let Err(e) = pty.write_input(&raw_bytes) {
                    tracing::warn!("PTY write_input (terminal focus): {e}");
                }
            }
        }
    }
    KeyAction::Handled
}
