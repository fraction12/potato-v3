//! Global application state.

use crate::agent::state_machine::AgentState;

/// The root state for the entire Potato application.
#[derive(Debug)]
pub struct AppState {
    /// Whether the application should exit on the next loop tick.
    pub should_quit: bool,
    /// Current phase of the AI agent.
    pub agent_state: AgentState,
    /// The active model name (e.g. "llama3").
    pub model: String,
    /// Path to the config file in use.
    pub config_path: String,
    /// Current input buffer (user is typing here).
    pub input_buffer: String,
    /// Conversation messages in the active session.
    pub messages: Vec<String>,
    /// Index of the currently active panel.
    pub active_panel: usize,
    /// Token usage counters: (prompt, completion).
    pub token_counts: (u64, u64),
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            should_quit: false,
            agent_state: AgentState::Idle,
            model: "llama3".to_string(),
            config_path: String::new(),
            input_buffer: String::new(),
            messages: Vec::new(),
            active_panel: 0,
            token_counts: (0, 0),
        }
    }
}
