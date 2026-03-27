//! Pure update function: maps (State, Message) → Action.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::{
    action::Action,
    message::{AgentEvent, Message},
    state::{AppState, ChatMessage, PendingApproval, ToolCallInfo, ToolCallStatus},
};
use crate::agent::state_machine::AgentState;

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

        // ── Agent events ──────────────────────────────────────────────────────
        Message::Agent(event) => handle_agent_event(state, event),
    }
}

// ── Key handler ───────────────────────────────────────────────────────────────

/// Dispatch a key event to the correct sub-handler based on app phase.
fn handle_key(state: &mut AppState, key: KeyEvent) -> Action {
    // Global quit bindings: Ctrl+Q or Ctrl+C.
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        match key.code {
            KeyCode::Char('q') | KeyCode::Char('c') => {
                state.should_quit = true;
                return Action::Quit;
            }
            _ => {}
        }
    }

    // If an approval is pending, only route approval keys.
    if state.pending_approval.is_some() {
        return handle_approval_key(state, key);
    }

    match key.code {
        // ── Scrollback ────────────────────────────────────────────────────────
        KeyCode::Up | KeyCode::Char('k') => {
            state.scroll_offset = state.scroll_offset.saturating_add(1);
            state.user_scrolled = true;
            Action::ScrollUp
        }
        KeyCode::Down | KeyCode::Char('j') => {
            if state.scroll_offset > 0 {
                state.scroll_offset -= 1;
                if state.scroll_offset == 0 {
                    state.user_scrolled = false;
                }
            }
            Action::ScrollDown
        }
        KeyCode::PageUp => {
            state.scroll_offset = state.scroll_offset.saturating_add(10);
            state.user_scrolled = true;
            Action::ScrollUp
        }
        KeyCode::PageDown => {
            if state.scroll_offset >= 10 {
                state.scroll_offset -= 10;
            } else {
                state.scroll_offset = 0;
                state.user_scrolled = false;
            }
            Action::ScrollDown
        }

        // ── Input cursor movement ─────────────────────────────────────────────
        KeyCode::Left => {
            state.input_cursor_left();
            Action::InputCursorLeft
        }
        KeyCode::Right => {
            state.input_cursor_right();
            Action::InputCursorRight
        }
        KeyCode::Home => {
            state.input_cursor_home();
            Action::InputCursorHome
        }
        KeyCode::End => {
            state.input_cursor_end();
            Action::InputCursorEnd
        }

        // ── Backspace ─────────────────────────────────────────────────────────
        KeyCode::Backspace => {
            state.input_backspace();
            Action::InputBackspace
        }

        // ── Submit ────────────────────────────────────────────────────────────
        KeyCode::Enter => {
            // Don't submit if agent is busy.
            if state.agent_state != AgentState::Idle {
                return Action::Noop;
            }
            let text = state.take_input().trim().to_string();
            if text.is_empty() {
                return Action::Noop;
            }
            // Push user message into the conversation immediately.
            state.push_message(ChatMessage::user(&text));
            // Reset scroll so the new message is visible.
            state.scroll_offset = 0;
            state.user_scrolled = false;
            Action::SendMessage(text)
        }

        // ── Character input ───────────────────────────────────────────────────
        KeyCode::Char(c) => {
            // Ignore Ctrl+<char> combinations that weren't caught above.
            if key.modifiers.contains(KeyModifiers::CONTROL) {
                return Action::Noop;
            }
            state.input_insert(c);
            Action::InputInsert(c)
        }

        KeyCode::Esc => Action::Noop,
        _ => Action::Noop,
    }
}

/// Handle keys when an approval prompt is active.
fn handle_approval_key(state: &mut AppState, key: KeyEvent) -> Action {
    match key.code {
        KeyCode::Char('y') | KeyCode::Char('Y') => {
            state.pending_approval = None;
            state.agent_state = AgentState::Idle;
            Action::ApproveToolCall
        }
        KeyCode::Char('n') | KeyCode::Char('N') => {
            state.pending_approval = None;
            state.agent_state = AgentState::Idle;
            Action::DenyToolCall
        }
        KeyCode::Char('a') | KeyCode::Char('A') => {
            state.pending_approval = None;
            state.agent_state = AgentState::Idle;
            Action::ApproveAllToolCalls
        }
        _ => Action::Noop,
    }
}

// ── Agent event handler ───────────────────────────────────────────────────────

/// Map an [`AgentEvent`] to a state mutation and optional action.
fn handle_agent_event(state: &mut AppState, event: AgentEvent) -> Action {
    match event {
        // Streaming token — append to the current assistant bubble.
        AgentEvent::Token(token) => {
            state.agent_state = AgentState::Thinking;
            state.append_token(&token);
            Action::Noop
        }

        // Response complete — finalize the assistant message.
        AgentEvent::ResponseComplete => {
            state.agent_state = AgentState::Idle;
            Action::Noop
        }

        // Tool approval required — show the approval bar.
        AgentEvent::ApprovalRequired { tool_name, args } => {
            state.agent_state = AgentState::Approval {
                tool_name: tool_name.clone(),
                args: args.clone(),
            };
            state.pending_approval = Some(PendingApproval {
                tool_name,
                args,
                preview: None,
            });
            Action::Noop
        }

        // Tool completed — find the matching tool card and update it.
        AgentEvent::ToolComplete { tool_name, output } => {
            // Walk messages in reverse to find the most recent running tool.
            for msg in state.messages.iter_mut().rev() {
                if let Some(ref mut tc) = msg.tool_call {
                    if tc.tool_name == tool_name && tc.status == ToolCallStatus::Running {
                        tc.status = ToolCallStatus::Done;
                        tc.output = Some(output);
                        break;
                    }
                }
            }
            // If agent was in ToolCall state, revert to Thinking (streaming continues).
            if matches!(state.agent_state, AgentState::ToolCall { .. }) {
                state.agent_state = AgentState::Thinking;
            }
            Action::Noop
        }

        // Tool call requested — transition to ToolCall state and add a card.
        AgentEvent::ToolCallRequested { tool_name, args: _ } => {
            state.agent_state = AgentState::ToolCall {
                tool_name: tool_name.clone(),
            };
            Action::Noop
        }

        // Error — push an error message and show it in the status bar.
        AgentEvent::Error(e) => {
            state.agent_state = AgentState::Error(e.clone());
            state.push_message(ChatMessage::error(&e));
            state.set_error(e, ERROR_DISMISS_TICKS);
            Action::Noop
        }
    }
}
