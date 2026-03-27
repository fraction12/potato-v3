//! Pure update function: maps (State, Message) → Action.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::{
    action::Action,
    message::Message,
    state::AppState,
};
use crate::ui::panels::PanelId;

/// Number of ticks before an error message is auto-dismissed.
const ERROR_DISMISS_TICKS: u32 = 40;

/// Process an incoming [`Message`] and mutate [`AppState`] accordingly.
///
/// Returns an [`Action`] describing any side-effects to be performed.
pub fn update(state: &mut AppState, msg: Message) -> Action {
    match msg {
        // ── Quit ──────────────────────────────────────────────────────────────
        Message::Quit => {
            state.should_quit = true;
            Action::Quit
        }

        // ── Periodic tick ─────────────────────────────────────────────────────
        Message::Tick => {
            state.tick_count = state.tick_count.wrapping_add(1);

            // Count down the error dismiss timer.
            if state.error_dismiss_ticks > 0 {
                state.error_dismiss_ticks -= 1;
                if state.error_dismiss_ticks == 0 {
                    state.error_message = None;
                }
            }

            Action::Noop
        }

        // ── Terminal resize ───────────────────────────────────────────────────
        Message::Resize(_, _) => Action::Noop,

        // ── Mouse (not handled yet) ───────────────────────────────────────────
        Message::Mouse(_) => Action::Noop,

        // ── Keyboard ─────────────────────────────────────────────────────────
        Message::Key(key) => handle_key(state, key),

        // ── Agent events (legacy path — main event loop handles these directly) ─
        Message::Agent(_) => Action::Noop,
    }
}

// ── Key handler ───────────────────────────────────────────────────────────────

/// Dispatch a key event to the correct sub-handler based on app phase.
fn handle_key(state: &mut AppState, key: KeyEvent) -> Action {
    // ── Global quit bindings: Ctrl+Q or Ctrl+C ────────────────────────────────
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        match key.code {
            KeyCode::Char('q') | KeyCode::Char('c') => {
                state.should_quit = true;
                return Action::Quit;
            }
            // ── Panel toggle: Ctrl+1/2/3/4 ────────────────────────────────────
            KeyCode::Char('1') => return Action::TogglePanel(PanelId::Chat),
            KeyCode::Char('2') => return Action::TogglePanel(PanelId::ToolOutput),
            KeyCode::Char('3') => return Action::TogglePanel(PanelId::FilePreview),
            KeyCode::Char('4') => return Action::TogglePanel(PanelId::Sessions),
            _ => {}
        }
    }

    // ── Focus cycling: Tab / Shift+Tab ────────────────────────────────────────
    if key.code == KeyCode::Tab {
        if key.modifiers.contains(KeyModifiers::SHIFT) {
            return Action::FocusPreviousPanel;
        }
        return Action::FocusNextPanel;
    }

    match key.code {
        // ── Scrollback ────────────────────────────────────────────────────────
        KeyCode::Up | KeyCode::Char('k') => Action::ScrollUp,
        KeyCode::Down | KeyCode::Char('j') => Action::ScrollDown,
        KeyCode::PageUp => Action::ScrollUp,
        KeyCode::PageDown => Action::ScrollDown,

        // ── Input cursor movement ─────────────────────────────────────────────
        KeyCode::Left => Action::InputCursorLeft,
        KeyCode::Right => Action::InputCursorRight,
        KeyCode::Home => Action::InputCursorHome,
        KeyCode::End => Action::InputCursorEnd,

        // ── Backspace ─────────────────────────────────────────────────────────
        KeyCode::Backspace => Action::InputBackspace,

        // ── Submit ────────────────────────────────────────────────────────────
        KeyCode::Enter => Action::Noop,

        // ── Character input ───────────────────────────────────────────────────
        KeyCode::Char(c) => {
            // Ignore Ctrl+<char> combinations that weren't caught above.
            if key.modifiers.contains(KeyModifiers::CONTROL) {
                return Action::Noop;
            }
            Action::InputInsert(c)
        }

        KeyCode::Esc => Action::Noop,
        _ => Action::Noop,
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::message::Message;

    /// Tab emits FocusNextPanel.
    #[test]
    fn test_tab_emits_focus_next() {
        let mut state = AppState::default();
        let key = KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE);
        let action = update(&mut state, Message::Key(key));
        assert_eq!(action, Action::FocusNextPanel);
    }

    /// Shift+Tab emits FocusPreviousPanel.
    #[test]
    fn test_shift_tab_emits_focus_prev() {
        let mut state = AppState::default();
        let key = KeyEvent::new(KeyCode::Tab, KeyModifiers::SHIFT);
        let action = update(&mut state, Message::Key(key));
        assert_eq!(action, Action::FocusPreviousPanel);
    }

    /// Ctrl+2 emits TogglePanel(ToolOutput).
    #[test]
    fn test_ctrl2_toggles_tool_output() {
        let mut state = AppState::default();
        let key = KeyEvent::new(KeyCode::Char('2'), KeyModifiers::CONTROL);
        let action = update(&mut state, Message::Key(key));
        assert_eq!(action, Action::TogglePanel(PanelId::ToolOutput));
    }

    /// Ctrl+1 emits TogglePanel(Chat).
    #[test]
    fn test_ctrl1_toggles_chat() {
        let mut state = AppState::default();
        let key = KeyEvent::new(KeyCode::Char('1'), KeyModifiers::CONTROL);
        let action = update(&mut state, Message::Key(key));
        assert_eq!(action, Action::TogglePanel(PanelId::Chat));
    }

    /// Ctrl+4 emits TogglePanel(Sessions).
    #[test]
    fn test_ctrl4_toggles_sessions() {
        let mut state = AppState::default();
        let key = KeyEvent::new(KeyCode::Char('4'), KeyModifiers::CONTROL);
        let action = update(&mut state, Message::Key(key));
        assert_eq!(action, Action::TogglePanel(PanelId::Sessions));
    }

    /// Panel-local keys (j/k) fire when the global handler routes them.
    #[test]
    fn test_panel_action_routing() {
        let mut state = AppState::default();
        // j in global context scrolls down
        let key = KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE);
        let action = update(&mut state, Message::Key(key));
        assert_eq!(action, Action::ScrollDown);
    }
}
