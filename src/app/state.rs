//! Global application state.

use chrono::{DateTime, Utc};
use crate::agent::state_machine::AgentState;

// ── Chat message types ────────────────────────────────────────────────────────

/// Role of a participant in the conversation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MessageRole {
    /// A message typed by the human user.
    User,
    /// A message streamed from the AI assistant.
    Assistant,
    /// A system-level notification or prompt.
    System,
    /// An error message to display in the chat.
    Error,
}

/// Status of a tool call embedded in an assistant message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolCallStatus {
    /// Tool is currently executing.
    Running,
    /// Tool completed successfully.
    Done,
    /// Tool failed with an error.
    Failed,
}

/// Metadata about a tool invocation attached to a message.
#[derive(Debug, Clone)]
pub struct ToolCallInfo {
    /// Name of the tool (e.g. `read_file`).
    pub tool_name: String,
    /// JSON-serialised arguments passed to the tool.
    pub args: String,
    /// Output produced by the tool, if it has finished.
    pub output: Option<String>,
    /// Current execution status.
    pub status: ToolCallStatus,
    /// When the tool call started (for duration display).
    pub started_at: DateTime<Utc>,
    /// Whether the tool card is expanded to show args/output.
    pub expanded: bool,
}

impl ToolCallInfo {
    /// Create a new running tool call.
    pub fn new(tool_name: impl Into<String>, args: impl Into<String>) -> Self {
        Self {
            tool_name: tool_name.into(),
            args: args.into(),
            output: None,
            status: ToolCallStatus::Running,
            started_at: Utc::now(),
            expanded: false,
        }
    }
}

/// A single message in the conversation history.
#[derive(Debug, Clone)]
pub struct ChatMessage {
    /// Who sent this message.
    pub role: MessageRole,
    /// The text content of the message.
    pub content: String,
    /// When this message was created.
    pub timestamp: DateTime<Utc>,
    /// Tool call attached to this message (assistant messages only).
    pub tool_call: Option<ToolCallInfo>,
}

impl ChatMessage {
    /// Create a user message with the current timestamp.
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: MessageRole::User,
            content: content.into(),
            timestamp: Utc::now(),
            tool_call: None,
        }
    }

    /// Create an assistant message with the current timestamp.
    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: MessageRole::Assistant,
            content: content.into(),
            timestamp: Utc::now(),
            tool_call: None,
        }
    }

    /// Create a system message.
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: MessageRole::System,
            content: content.into(),
            timestamp: Utc::now(),
            tool_call: None,
        }
    }

    /// Create an error message.
    pub fn error(content: impl Into<String>) -> Self {
        Self {
            role: MessageRole::Error,
            content: content.into(),
            timestamp: Utc::now(),
            tool_call: None,
        }
    }
}

// ── UI phase ──────────────────────────────────────────────────────────────────

/// High-level phase of the UI, controlling what is shown.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum UiPhase {
    /// Welcome / empty state, no messages yet.
    #[default]
    Welcome,
    /// Active conversation in progress.
    Active,
}

// ── Pending approval ──────────────────────────────────────────────────────────

/// Data for a tool call awaiting user approval.
#[derive(Debug, Clone)]
pub struct PendingApproval {
    /// Name of the tool requesting approval.
    pub tool_name: String,
    /// JSON-serialised arguments.
    pub args: String,
    /// Optional diff or preview string for write/edit operations.
    pub preview: Option<String>,
}

// ── AppState ──────────────────────────────────────────────────────────────────

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
    /// Cursor byte-index within `input_buffer`.
    pub input_cursor: usize,
    /// Structured conversation messages.
    pub messages: Vec<ChatMessage>,
    /// Index of the currently active panel.
    pub active_panel: usize,
    /// Token usage counters: (prompt, completion).
    pub token_counts: (u64, u64),
    /// Vertical scroll offset for the chat view (lines from the bottom).
    pub scroll_offset: usize,
    /// Whether the user has manually scrolled up (suppresses auto-scroll).
    pub user_scrolled: bool,
    /// High-level UI phase.
    pub ui_phase: UiPhase,
    /// Tool call awaiting user approval, if any.
    pub pending_approval: Option<PendingApproval>,
    /// Transient error message shown in the status bar.
    pub error_message: Option<String>,
    /// Remaining ticks before `error_message` is cleared.
    pub error_dismiss_ticks: u32,
    /// Tick counter used for spinner animation frames.
    pub tick_count: u64,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            should_quit: false,
            agent_state: AgentState::Idle,
            model: "llama3".to_string(),
            config_path: String::new(),
            input_buffer: String::new(),
            input_cursor: 0,
            messages: Vec::new(),
            active_panel: 0,
            token_counts: (0, 0),
            scroll_offset: 0,
            user_scrolled: false,
            ui_phase: UiPhase::Welcome,
            pending_approval: None,
            error_message: None,
            error_dismiss_ticks: 0,
            tick_count: 0,
        }
    }
}

impl AppState {
    /// Push a message into the conversation and switch to `Active` phase.
    pub fn push_message(&mut self, msg: ChatMessage) {
        self.messages.push(msg);
        self.ui_phase = UiPhase::Active;
        // Auto-scroll to bottom unless the user has manually scrolled up.
        if !self.user_scrolled {
            self.scroll_offset = 0;
        }
    }

    /// Append a token to the last assistant message, or create one.
    pub fn append_token(&mut self, token: &str) {
        match self.messages.last_mut() {
            Some(m) if m.role == MessageRole::Assistant => {
                m.content.push_str(token);
            }
            _ => {
                self.messages.push(ChatMessage::assistant(token));
                self.ui_phase = UiPhase::Active;
            }
        }
        if !self.user_scrolled {
            self.scroll_offset = 0;
        }
    }

    /// Set a transient error message that auto-dismisses after `ticks` ticks.
    pub fn set_error(&mut self, msg: impl Into<String>, ticks: u32) {
        self.error_message = Some(msg.into());
        self.error_dismiss_ticks = ticks;
    }

    /// Insert a character at the cursor position.
    pub fn input_insert(&mut self, c: char) {
        self.input_buffer.insert(self.input_cursor, c);
        self.input_cursor += c.len_utf8();
    }

    /// Delete the character before the cursor (backspace).
    pub fn input_backspace(&mut self) {
        if self.input_cursor == 0 {
            return;
        }
        // Find the previous char boundary.
        let before = &self.input_buffer[..self.input_cursor];
        if let Some(c) = before.chars().next_back() {
            self.input_cursor -= c.len_utf8();
            self.input_buffer.remove(self.input_cursor);
        }
    }

    /// Move the cursor one character to the left.
    pub fn input_cursor_left(&mut self) {
        if self.input_cursor == 0 {
            return;
        }
        let before = &self.input_buffer[..self.input_cursor];
        if let Some(c) = before.chars().next_back() {
            self.input_cursor -= c.len_utf8();
        }
    }

    /// Move the cursor one character to the right.
    pub fn input_cursor_right(&mut self) {
        if self.input_cursor >= self.input_buffer.len() {
            return;
        }
        if let Some(c) = self.input_buffer[self.input_cursor..].chars().next() {
            self.input_cursor += c.len_utf8();
        }
    }

    /// Move the cursor to the beginning of the input.
    pub fn input_cursor_home(&mut self) {
        self.input_cursor = 0;
    }

    /// Move the cursor to the end of the input.
    pub fn input_cursor_end(&mut self) {
        self.input_cursor = self.input_buffer.len();
    }

    /// Take the current input buffer, clear it, and return the text.
    pub fn take_input(&mut self) -> String {
        self.input_cursor = 0;
        std::mem::take(&mut self.input_buffer)
    }
}
