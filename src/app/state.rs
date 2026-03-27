//! Global application state — dashboard-first architecture.
//!
//! The app boots to [`AppScreen::Dashboard`] where the user picks an agent,
//! then transitions to [`AppScreen::Session`] hosting the live PTY session.

use std::path::PathBuf;

use chrono::{DateTime, Utc};

use crate::app::agent_state::AgentState;
use crate::metrics::SessionMetrics;
use crate::ui::panels::PanelId;

// ── LayoutPreset (kept for existing UI compatibility) ─────────────────────────

/// High-level layout modes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LayoutPreset {
    #[default]
    Wide,
    Sidebar,
    Minimal,
}

// ── Existing chat message types (kept for UI compatibility) ───────────────────

/// Role of a participant in the conversation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MessageRole {
    User,
    Assistant,
    System,
    Error,
}

/// Status of a tool call embedded in an assistant message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolCallStatus {
    Running,
    Done,
    Failed,
}

/// Metadata about a tool invocation attached to a message.
#[derive(Debug, Clone)]
pub struct ToolCallInfo {
    pub tool_name: String,
    pub args: String,
    pub output: Option<String>,
    pub status: ToolCallStatus,
    pub started_at: DateTime<Utc>,
    pub expanded: bool,
}

impl ToolCallInfo {
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
    pub role: MessageRole,
    pub content: String,
    pub timestamp: DateTime<Utc>,
    pub tool_call: Option<ToolCallInfo>,
}

impl ChatMessage {
    pub fn user(content: impl Into<String>) -> Self {
        Self { role: MessageRole::User, content: content.into(), timestamp: Utc::now(), tool_call: None }
    }
    pub fn assistant(content: impl Into<String>) -> Self {
        Self { role: MessageRole::Assistant, content: content.into(), timestamp: Utc::now(), tool_call: None }
    }
    pub fn system(content: impl Into<String>) -> Self {
        Self { role: MessageRole::System, content: content.into(), timestamp: Utc::now(), tool_call: None }
    }
    pub fn error(content: impl Into<String>) -> Self {
        Self { role: MessageRole::Error, content: content.into(), timestamp: Utc::now(), tool_call: None }
    }
}

/// High-level phase of the UI.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum UiPhase {
    #[default]
    Welcome,
    Active,
}

/// Data for a tool call awaiting user approval.
#[derive(Debug, Clone)]
pub struct PendingApproval {
    pub tool_name: String,
    pub args: String,
    pub preview: Option<String>,
}

// ── Dashboard types ───────────────────────────────────────────────────────────

/// Information about a detectable agent.
#[derive(Debug, Clone)]
pub struct AgentInfo {
    /// Display name (e.g. `"Claude Code"`).
    pub name: String,
    /// Adapter identifier (e.g. `"claude"`).
    pub adapter: String,
    /// Resolved binary path, if the agent is installed.
    pub binary_path: Option<PathBuf>,
    /// Whether the agent binary was found on the system.
    pub available: bool,
}

/// A brief summary of a past session, shown in the dashboard.
#[derive(Debug, Clone)]
pub struct SessionSummary {
    pub session_id: String,
    pub agent_name: String,
    pub started_at: DateTime<Utc>,
    pub total_cost_usd: f64,
    pub turn_count: u64,
}

/// Which column of the dashboard holds focus.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum DashboardFocus {
    /// The agent list on the left.
    #[default]
    AgentList,
    /// The recent sessions list on the right.
    SessionList,
}

/// State for the dashboard screen.
#[derive(Debug, Clone, Default)]
pub struct DashboardState {
    /// Agents detected on the system.
    pub available_agents: Vec<AgentInfo>,
    /// Recent sessions loaded from the store.
    pub recent_sessions: Vec<SessionSummary>,
    /// Selected row in the agent list.
    pub selected_agent: usize,
    /// Selected row in the sessions list.
    pub selected_session: usize,
    /// Which panel has keyboard focus.
    pub focus: DashboardFocus,
}

// ── Session types ─────────────────────────────────────────────────────────────

/// A transcript entry (message or tool event) shown in the session view.
#[derive(Debug, Clone)]
pub struct TranscriptEntry {
    pub role: MessageRole,
    pub content: String,
    pub timestamp: DateTime<Utc>,
    pub tool_call: Option<ToolCallInfo>,
}

impl TranscriptEntry {
    pub fn assistant(content: impl Into<String>) -> Self {
        Self { role: MessageRole::Assistant, content: content.into(), timestamp: Utc::now(), tool_call: None }
    }
    pub fn user(content: impl Into<String>) -> Self {
        Self { role: MessageRole::User, content: content.into(), timestamp: Utc::now(), tool_call: None }
    }
    pub fn system(content: impl Into<String>) -> Self {
        Self { role: MessageRole::System, content: content.into(), timestamp: Utc::now(), tool_call: None }
    }
}

/// A record of a single tool invocation in the session timeline.
#[derive(Debug, Clone)]
pub struct ToolCallRecord {
    pub id: String,
    pub name: String,
    pub input: serde_json::Value,
    pub output: Option<String>,
    pub started_at: DateTime<Utc>,
    pub duration_ms: Option<u64>,
    pub success: Option<bool>,
}

/// Current execution status of the active agent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentStatus {
    Starting,
    Idle,
    Thinking,
    RunningTool { name: String },
    WaitingApproval { tool_name: String },
    Exited { code: Option<i32> },
    Error { message: String },
}

impl Default for AgentStatus {
    fn default() -> Self { Self::Idle }
}

impl AgentStatus {
    /// Returns true if the agent is actively doing something.
    pub fn is_active(&self) -> bool {
        matches!(self, Self::Starting | Self::Thinking | Self::RunningTool { .. } | Self::WaitingApproval { .. })
    }
}

/// Pending approval for a tool call awaiting user decision.
#[derive(Debug, Clone)]
pub struct PendingApprovalSession {
    pub tool_id: String,
    pub tool_name: String,
    pub input: serde_json::Value,
}

/// State for an active agent session screen.
#[derive(Debug)]
pub struct SessionState {
    pub session_id: String,
    pub agent_name: String,
    pub transcript: Vec<TranscriptEntry>,
    pub tool_calls: Vec<ToolCallRecord>,
    pub metrics: SessionMetrics,
    pub approval_pending: Option<PendingApprovalSession>,
    pub status: AgentStatus,
    pub input_buffer: String,
    pub scroll_offset: u16,
    pub input_cursor: usize,
    pub tick_count: u64,
}

impl SessionState {
    pub fn new(session_id: impl Into<String>, agent_name: impl Into<String>) -> Self {
        Self {
            session_id: session_id.into(),
            agent_name: agent_name.into(),
            transcript: Vec::new(),
            tool_calls: Vec::new(),
            metrics: SessionMetrics::default(),
            approval_pending: None,
            status: AgentStatus::Starting,
            input_buffer: String::new(),
            scroll_offset: 0,
            input_cursor: 0,
            tick_count: 0,
        }
    }
}

// ── AppScreen ─────────────────────────────────────────────────────────────────

/// The top-level screen the application is currently showing.
pub enum AppScreen {
    Dashboard(DashboardState),
    Session(SessionState),
}

impl Default for AppScreen {
    fn default() -> Self {
        Self::Dashboard(DashboardState::default())
    }
}

// ── AppState ──────────────────────────────────────────────────────────────────

/// Root state for the Potato application.
#[derive(Debug)]
pub struct AppState {
    /// Whether the application should exit on the next loop tick.
    pub should_quit: bool,

    // ── Active screen ─────────────────────────────────────────────────────────
    /// The current top-level screen.
    ///
    /// Use pattern matching to access dashboard or session state.
    pub screen: AppScreen,

    // ── Shared / config ───────────────────────────────────────────────────────
    /// Active model name (overridable per session).
    pub model: String,
    /// Path to the loaded config file.
    pub config_path: String,

    // ── Legacy fields kept for existing UI code compatibility ─────────────────
    /// Legacy Ollama-era agent state (kept for UI compatibility).
    pub agent_state: AgentState,
    /// Current input buffer (used by legacy render path).
    pub input_buffer: String,
    pub input_cursor: usize,
    pub messages: Vec<ChatMessage>,
    pub active_panel: usize,
    pub token_counts: (u64, u64),
    pub scroll_offset: usize,
    pub user_scrolled: bool,
    pub ui_phase: UiPhase,
    pub pending_approval: Option<PendingApproval>,
    pub error_message: Option<String>,
    pub error_dismiss_ticks: u32,
    pub tick_count: u64,
    pub focused_panel: PanelId,
    pub visible_panels: Vec<PanelId>,
    pub layout_preset: LayoutPreset,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            should_quit: false,
            screen: AppScreen::default(),
            model: "claude".to_string(),
            config_path: String::new(),
            agent_state: AgentState::default(),
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
            focused_panel: PanelId::Chat,
            visible_panels: vec![PanelId::Chat],
            layout_preset: LayoutPreset::Wide,
        }
    }
}

impl AppState {
    // ── Dashboard helpers ─────────────────────────────────────────────────────

    /// Return a mutable reference to the dashboard state if active.
    pub fn dashboard_mut(&mut self) -> Option<&mut DashboardState> {
        if let AppScreen::Dashboard(ref mut d) = self.screen { Some(d) } else { None }
    }

    /// Return a reference to the dashboard state if active.
    pub fn dashboard(&self) -> Option<&DashboardState> {
        if let AppScreen::Dashboard(ref d) = self.screen { Some(d) } else { None }
    }

    /// Transition to a session screen.
    pub fn enter_session(&mut self, session_id: impl Into<String>, agent_name: impl Into<String>) {
        self.screen = AppScreen::Session(SessionState::new(session_id, agent_name));
    }

    /// Return a mutable reference to the session state if active.
    pub fn session_mut(&mut self) -> Option<&mut SessionState> {
        if let AppScreen::Session(ref mut s) = self.screen { Some(s) } else { None }
    }

    /// Return a reference to the session state if active.
    pub fn session(&self) -> Option<&SessionState> {
        if let AppScreen::Session(ref s) = self.screen { Some(s) } else { None }
    }

    // ── Legacy helpers (kept for existing UI code) ────────────────────────────

    pub fn push_message(&mut self, msg: ChatMessage) {
        self.messages.push(msg);
        self.ui_phase = UiPhase::Active;
        if !self.user_scrolled { self.scroll_offset = 0; }
    }

    pub fn append_token(&mut self, token: &str) {
        match self.messages.last_mut() {
            Some(m) if m.role == MessageRole::Assistant => { m.content.push_str(token); }
            _ => {
                self.messages.push(ChatMessage::assistant(token));
                self.ui_phase = UiPhase::Active;
            }
        }
        if !self.user_scrolled { self.scroll_offset = 0; }
    }

    pub fn set_error(&mut self, msg: impl Into<String>, ticks: u32) {
        self.error_message = Some(msg.into());
        self.error_dismiss_ticks = ticks;
    }

    pub fn input_insert(&mut self, c: char) {
        self.input_buffer.insert(self.input_cursor, c);
        self.input_cursor += c.len_utf8();
    }

    pub fn input_backspace(&mut self) {
        if self.input_cursor == 0 { return; }
        let before = &self.input_buffer[..self.input_cursor];
        if let Some(c) = before.chars().next_back() {
            self.input_cursor -= c.len_utf8();
            self.input_buffer.remove(self.input_cursor);
        }
    }

    pub fn input_cursor_left(&mut self) {
        if self.input_cursor == 0 { return; }
        let before = &self.input_buffer[..self.input_cursor];
        if let Some(c) = before.chars().next_back() { self.input_cursor -= c.len_utf8(); }
    }

    pub fn input_cursor_right(&mut self) {
        if self.input_cursor >= self.input_buffer.len() { return; }
        if let Some(c) = self.input_buffer[self.input_cursor..].chars().next() {
            self.input_cursor += c.len_utf8();
        }
    }

    pub fn input_cursor_home(&mut self) { self.input_cursor = 0; }
    pub fn input_cursor_end(&mut self) { self.input_cursor = self.input_buffer.len(); }

    pub fn take_input(&mut self) -> String {
        self.input_cursor = 0;
        std::mem::take(&mut self.input_buffer)
    }
}

// ── std::fmt::Debug for AppScreen ─────────────────────────────────────────────

impl std::fmt::Debug for AppScreen {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AppScreen::Dashboard(_) => write!(f, "AppScreen::Dashboard(…)"),
            AppScreen::Session(_) => write!(f, "AppScreen::Session(…)"),
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_screen_is_dashboard() {
        let state = AppState::default();
        assert!(state.dashboard().is_some());
        assert!(state.session().is_none());
    }

    #[test]
    fn enter_session_transitions_screen() {
        let mut state = AppState::default();
        state.enter_session("s-1", "claude");
        assert!(state.session().is_some());
        assert!(state.dashboard().is_none());
        let s = state.session().unwrap();
        assert_eq!(s.session_id, "s-1");
        assert_eq!(s.agent_name, "claude");
    }

    #[test]
    fn session_state_defaults() {
        let s = SessionState::new("id", "agent");
        assert_eq!(s.status, AgentStatus::Starting);
        assert!(s.transcript.is_empty());
        assert_eq!(s.scroll_offset, 0);
    }

    #[test]
    fn dashboard_state_defaults() {
        let d = DashboardState::default();
        assert!(d.available_agents.is_empty());
        assert!(d.recent_sessions.is_empty());
        assert_eq!(d.selected_agent, 0);
        assert_eq!(d.focus, DashboardFocus::AgentList);
    }

    #[test]
    fn agent_status_is_active() {
        assert!(AgentStatus::Thinking.is_active());
        assert!(!AgentStatus::Idle.is_active());
        assert!(!AgentStatus::Exited { code: Some(0) }.is_active());
    }

    #[test]
    fn input_insert_and_backspace() {
        let mut state = AppState::default();
        state.input_insert('h');
        state.input_insert('i');
        assert_eq!(state.input_buffer, "hi");
        state.input_backspace();
        assert_eq!(state.input_buffer, "h");
    }

    #[test]
    fn take_input_clears_buffer() {
        let mut state = AppState::default();
        state.input_insert('x');
        let taken = state.take_input();
        assert_eq!(taken, "x");
        assert!(state.input_buffer.is_empty());
        assert_eq!(state.input_cursor, 0);
    }
}
