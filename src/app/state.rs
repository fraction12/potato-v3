//! Global application state — dashboard-first architecture.
//!
//! The app boots to [`AppScreen::Dashboard`] where the user picks an agent,
//! then transitions to [`AppScreen::Session`] hosting the live PTY session.

use std::path::PathBuf;
use std::sync::Arc;

use chrono::{DateTime, Utc};

use crate::metrics::SessionMetrics;
use crate::session::store::{SessionInfo, SessionStore};
use crate::ui::focus::FocusRing;
use crate::ui::layout::LayoutManager;
use crate::ui::layout::LayoutPreset as NewLayoutPreset;
use crate::ui::panels::PanelId;
use crate::ui::panels::chat::ChatPanel;
use crate::ui::panels::tool_output::ToolOutputPanel;

// ── LayoutPreset (kept for existing UI compatibility) ─────────────────────────

/// High-level layout modes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LayoutPreset {
    #[default]
    Wide,
    Sidebar,
    Minimal,
}

// ── Message role (shared between transcript and widgets) ──────────────────────

/// Role of a participant in the conversation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MessageRole {
    User,
    Assistant,
    System,
    Error,
}

// ── Tool call types (used in session transcript) ──────────────────────────────

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
    /// The left menu.
    #[default]
    Menu,
    /// A sub-list inside the detail pane (e.g. recent sessions, roles).
    Detail,
}

/// Items in the left menu rail.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DashboardMenuItem {
    RoastPotato,
    DefineRoles,
    Integrations,
    Settings,
}

impl DashboardMenuItem {
    pub const ALL: [Self; 4] = [
        Self::RoastPotato,
        Self::DefineRoles,
        Self::Integrations,
        Self::Settings,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::RoastPotato => "Roast Potato",
            Self::DefineRoles => "Define Roles",
            Self::Integrations => "Integrations",
            Self::Settings => "Settings",
        }
    }
}

/// A role definition for a pane.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RoleDefinition {
    pub name: String,
    pub prompt: String,
}

/// Inline input mode for the dashboard (e.g. adding a role).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum DashboardInput {
    /// No inline input active.
    #[default]
    None,
    /// Typing the name for a new role.
    RoleName(String),
    /// Typing the prompt for a new role (name already captured).
    RolePrompt { name: String, prompt: String },
}

/// State for the dashboard screen.
#[derive(Debug, Clone, Default)]
pub struct DashboardState {
    /// Agents detected on the system.
    pub available_agents: Vec<AgentInfo>,
    /// Recent sessions loaded from the store.
    pub recent_sessions: Vec<SessionSummary>,
    /// Selected menu item index (into `DashboardMenuItem::ALL`).
    pub selected_menu: usize,
    /// Selected row inside the detail pane (sessions list, etc.).
    pub selected_detail: usize,
    /// Which panel has keyboard focus.
    pub focus: DashboardFocus,
    /// Role definitions for panes.
    pub roles: Vec<RoleDefinition>,
    /// Selected agent index (for the agent list on the old path — kept for compat).
    pub selected_agent: usize,
    /// Inline input state (e.g. adding a new role).
    pub input: DashboardInput,
}

// ── Cockpit focus ─────────────────────────────────────────────────────────────

/// Which panel in the cockpit session screen holds keyboard focus.
///
/// Tab order: Sessions → Input → Terminal → Sidebar → (wrap).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CockpitFocus {
    /// Left rail top — agent picker (e.g. "Claude").
    Agents,
    /// Left rail bottom — historical session list.
    Sessions,
    /// Bottom-center — Potato-owned text input bar.
    #[default]
    Input,
    /// Center — the embedded PTY terminal viewport.
    Terminal,
    /// Right rail — metrics / tools / skills sidebar.
    Sidebar,
}

impl CockpitFocus {
    /// Advance to the next focus in the ring (Tab).
    pub fn next(self) -> Self {
        match self {
            Self::Agents   => Self::Sessions,
            Self::Sessions => Self::Input,
            Self::Input    => Self::Terminal,
            Self::Terminal => Self::Sidebar,
            Self::Sidebar  => Self::Agents,
        }
    }

    /// Retreat to the previous focus in the ring (Shift+Tab).
    pub fn prev(self) -> Self {
        match self {
            Self::Agents   => Self::Sidebar,
            Self::Sessions => Self::Agents,
            Self::Input    => Self::Sessions,
            Self::Terminal => Self::Input,
            Self::Sidebar  => Self::Terminal,
        }
    }
}

// ── Overlay ───────────────────────────────────────────────────────────────────

/// Which full-screen modal overlay (if any) is currently active.
///
/// The overlay renders on top of the cockpit and captures all keyboard input
/// until dismissed with `?` or `Esc`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Overlay {
    /// Keyboard shortcuts / commands reference sheet.
    Help,
    /// Session picker (stub — full implementation in a later phase).
    Sessions,
    /// Agent picker — select which agent to launch in a new pane.
    AgentPicker,
}

// ── AgentPickerState ──────────────────────────────────────────────────────────

/// State for the agent picker overlay.
#[derive(Debug, Clone, Default)]
pub struct AgentPickerState {
    /// Currently highlighted row index.
    pub selected: usize,
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
    /// How many lines the transcript view is scrolled up from the bottom.
    /// 0 = pinned to the bottom (auto-scroll). Increases as user scrolls up.
    pub scroll_offset: u16,
    /// Whether the user has manually scrolled up. When true, auto-scroll is
    /// suppressed until the user scrolls back to the bottom.
    pub user_scrolled: bool,
    pub input_cursor: usize,
    pub tick_count: u64,
    /// The Claude-native session id received from the last [`AgentEvent::SessionBound`] event.
    ///
    /// Pass this as `--resume <id>` when spawning the next turn so Claude can
    /// continue the conversation thread. `None` until the first turn completes.
    pub claude_session_id: Option<String>,

    /// Cumulative token count for this session (updated from metrics events).
    pub tokens_used: u64,

    /// Which cockpit panel currently holds keyboard focus.
    ///
    /// Default: `Input` — the user can start typing immediately.
    pub cockpit_focus: CockpitFocus,

    /// Index of the selected agent in the left-rail agents picker.
    pub selected_agent: usize,

    /// Index of the selected session in the left-rail sessions list.
    pub selected_session: usize,

    /// Scrollback offset for the embedded Claude terminal viewport.
    ///
    /// `0` means live-follow at the bottom. Larger values mean the user has
    /// scrolled up in the Claude pane.
    pub terminal_scroll: usize,

    /// Currently active modal overlay, if any.
    ///
    /// When `Some`, the overlay is rendered over the cockpit and all key
    /// events are dispatched to it instead of the cockpit widgets.
    pub overlay: Option<Overlay>,

    /// Index of the currently highlighted item in the slash-command autocomplete
    /// popup. Reset to 0 whenever the input buffer changes or is cleared.
    pub command_selected: usize,

    /// State for the agent picker overlay.
    pub agent_picker: AgentPickerState,
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
            user_scrolled: false,
            input_cursor: 0,
            tick_count: 0,
            claude_session_id: None,
            tokens_used: 0,
            cockpit_focus: CockpitFocus::Input,
            selected_agent: 0,
            selected_session: 0,
            terminal_scroll: 0,
            overlay: None,
            command_selected: 0,
            agent_picker: AgentPickerState::default(),
        }
    }

    pub fn scroll_terminal_up(&mut self, lines: usize) {
        self.terminal_scroll = self.terminal_scroll.saturating_add(lines);
    }

    pub fn scroll_terminal_down(&mut self, lines: usize) {
        self.terminal_scroll = self.terminal_scroll.saturating_sub(lines);
    }

    pub fn reset_terminal_scroll(&mut self) {
        self.terminal_scroll = 0;
    }

    pub fn terminal_is_live(&self) -> bool {
        self.terminal_scroll == 0
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
    /// Active model name (shown in status bar; passed through from CLI --model).
    pub model: String,
    /// Path to the loaded config file.
    pub config_path: String,

    // ── Error / notification state ────────────────────────────────────────────
    pub error_message: Option<String>,
    pub error_dismiss_ticks: u32,
    pub tick_count: u64,

    // ── Phase-3 panel system ──────────────────────────────────────────────────
    /// Composable layout manager (Phase-3).
    pub layout_manager: LayoutManager,
    /// Focus ring for the session panel system (Phase-3).
    pub focus_ring: FocusRing,

    // ── Phase-3 panels ────────────────────────────────────────────────────────
    /// Chat panel — owns transcript scroll and search state.
    pub chat_panel: ChatPanel,
    /// Tool output panel — owns the collapsible tool execution timeline.
    pub tool_output_panel: ToolOutputPanel,

    // ── Multi-pane session management (cockpit mode) ──────────────────────────
    /// Manages up to 2 simultaneous Claude session panes, each with its own
    /// PTY and log tracker.
    pub panes: crate::app::pane::PaneManager,

    // ── Session store (cockpit persistence) ───────────────────────────────────
    /// Shared SQLite session store. `Arc` so it can be passed to async helpers
    /// without borrowing AppState.
    pub store: Option<Arc<SessionStore>>,

    /// Cached list of sessions for the left rail (refreshed periodically).
    pub rail_sessions: Vec<SessionInfo>,

    /// Unix timestamp of the last left-rail refresh (seconds).
    pub last_rail_refresh: i64,

    /// Number of events already persisted for the active PTY session.
    /// Used to detect newly written JSONL lines without double-inserting.
    pub persisted_event_count: u64,

    /// Path to the Unix domain socket used by the MCP bridge.
    ///
    /// `Some` once `McpBridge::start()` has been called in `main()`.
    /// Passed to each pane's PTY subprocess as `POTATO_SOCKET`.
    pub mcp_socket_path: Option<std::path::PathBuf>,

    /// Shared inter-session coordination state (messages, tasks, roles, context).
    ///
    /// Arc<Mutex<>> so it can be shared with the MCP bridge and read from UI.
    pub inter_session_state: Option<Arc<std::sync::Mutex<crate::mcp::state::InterSessionState>>>,

    /// Receiver for MCP injection requests (messages to push into pane PTYs).
    ///
    /// The bridge sends `InjectRequest`s after `send_message`; the main loop
    /// drains this each tick and writes into the target pane's PTY.
    pub inject_rx: Option<tokio::sync::mpsc::UnboundedReceiver<crate::mcp::injection::InjectRequest>>,

    /// OpenSpec watcher — tracks `.openspec/backlog.yaml` for live task data.
    pub openspec: Option<crate::openspec::OpenSpecWatcher>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            should_quit: false,
            screen: AppScreen::default(),
            model: "claude".to_string(),
            config_path: String::new(),
            error_message: None,
            error_dismiss_ticks: 0,
            tick_count: 0,
            layout_manager: LayoutManager::new(NewLayoutPreset::Sidebar),
            focus_ring: FocusRing::new(vec![PanelId::Chat, PanelId::ToolOutput]),
            chat_panel: ChatPanel::new(Vec::new()),
            tool_output_panel: ToolOutputPanel::new(),
            panes: crate::app::pane::PaneManager::new(),

            store: None,
            rail_sessions: Vec::new(),
            last_rail_refresh: 0,
            persisted_event_count: 0,
            mcp_socket_path: None,
            inter_session_state: None,
            inject_rx: None,
            openspec: None,
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

    // ── Error helpers ─────────────────────────────────────────────────────────

    pub fn set_error(&mut self, msg: impl Into<String>, ticks: u32) {
        self.error_message = Some(msg.into());
        self.error_dismiss_ticks = ticks;
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
        assert!(!s.user_scrolled);
        assert_eq!(s.terminal_scroll, 0);
        assert!(s.terminal_is_live());
    }

    #[test]
    fn terminal_scroll_saturates_and_resets() {
        let mut s = SessionState::new("id", "agent");
        s.scroll_terminal_up(24);
        s.scroll_terminal_up(10);
        assert_eq!(s.terminal_scroll, 34);
        assert!(!s.terminal_is_live());

        s.scroll_terminal_down(9);
        assert_eq!(s.terminal_scroll, 25);

        s.scroll_terminal_down(999);
        assert_eq!(s.terminal_scroll, 0);
        assert!(s.terminal_is_live());

        s.scroll_terminal_up(12);
        s.reset_terminal_scroll();
        assert_eq!(s.terminal_scroll, 0);
    }

    #[test]
    fn dashboard_state_defaults() {
        let d = DashboardState::default();
        assert!(d.available_agents.is_empty());
        assert!(d.recent_sessions.is_empty());
        assert_eq!(d.selected_agent, 0);
        assert_eq!(d.focus, DashboardFocus::Menu);
    }

    #[test]
    fn agent_status_is_active() {
        assert!(AgentStatus::Thinking.is_active());
        assert!(!AgentStatus::Idle.is_active());
        assert!(!AgentStatus::Exited { code: Some(0) }.is_active());
    }

    #[test]
    fn session_state_overlay_defaults_to_none() {
        let s = SessionState::new("id", "agent");
        assert!(s.overlay.is_none());
    }

    #[test]
    fn session_state_command_selected_defaults_to_zero() {
        let s = SessionState::new("id", "agent");
        assert_eq!(s.command_selected, 0);
    }

    #[test]
    fn overlay_enum_variants_are_distinct() {
        assert_ne!(Overlay::Help, Overlay::Sessions);
        assert_ne!(Overlay::Help, Overlay::AgentPicker);
        assert_ne!(Overlay::Sessions, Overlay::AgentPicker);
    }

    #[test]
    fn session_state_overlay_can_be_set_and_cleared() {
        let mut s = SessionState::new("id", "agent");
        s.overlay = Some(Overlay::Help);
        assert_eq!(s.overlay, Some(Overlay::Help));

        s.overlay = Some(Overlay::Sessions);
        assert_eq!(s.overlay, Some(Overlay::Sessions));

        s.overlay = Some(Overlay::AgentPicker);
        assert_eq!(s.overlay, Some(Overlay::AgentPicker));

        s.overlay = None;
        assert!(s.overlay.is_none());
    }

    #[test]
    fn agent_picker_state_defaults_to_selected_zero() {
        let s = SessionState::new("id", "agent");
        assert_eq!(s.agent_picker.selected, 0);
    }

    #[test]
    fn agent_picker_state_selected_can_be_changed() {
        let mut state = AgentPickerState::default();
        state.selected = 2;
        assert_eq!(state.selected, 2);
    }
}
