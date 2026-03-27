//! Pure update function: maps (State, Message) → Action.

use crossterm::event::KeyCode;

use super::{
    action::Action,
    message::{AgentEvent, Message},
    state::AppState,
};

/// Process an incoming [`Message`] and mutate [`AppState`] accordingly.
///
/// Returns an [`Action`] describing any side-effects to be performed.
pub fn update(state: &mut AppState, msg: Message) -> Action {
    match msg {
        Message::Quit => {
            state.should_quit = true;
            Action::Quit
        }

        Message::Tick => Action::Noop,

        Message::Resize(_, _) => Action::Noop,

        Message::Key(key) => match key.code {
            KeyCode::Char('q') => {
                state.should_quit = true;
                Action::Quit
            }
            KeyCode::Enter => {
                let text = state.input_buffer.trim().to_string();
                if text.is_empty() {
                    Action::Noop
                } else {
                    state.input_buffer.clear();
                    Action::SendMessage(text)
                }
            }
            KeyCode::Char(c) => {
                state.input_buffer.push(c);
                Action::Noop
            }
            KeyCode::Backspace => {
                state.input_buffer.pop();
                Action::Noop
            }
            KeyCode::Esc => Action::Noop,
            _ => Action::Noop,
        },

        Message::Mouse(_) => Action::Noop,

        Message::Agent(event) => match event {
            AgentEvent::Token(token) => {
                if let Some(last) = state.messages.last_mut() {
                    last.push_str(&token);
                } else {
                    state.messages.push(token);
                }
                Action::Noop
            }
            AgentEvent::ResponseComplete => Action::Noop,
            AgentEvent::ApprovalRequired { .. } => Action::Noop,
            AgentEvent::ToolComplete { .. } => Action::Noop,
            AgentEvent::Error(e) => {
                state.messages.push(format!("[error] {}", e));
                Action::Noop
            }
        },
    }
}
