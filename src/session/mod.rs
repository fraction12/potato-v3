//! Session management — persistence, history, export, and provider-specific log handling.

pub mod discovery;
pub mod export;
pub mod history;
pub mod logs;
pub mod store;

pub use discovery::{
    HistoricalSessionDiscovery, discover_claude_historical_sessions,
    discover_codex_historical_sessions, discover_historical_sessions,
    discover_historical_sessions_for,
};
pub use history::MessageHistory;
pub use logs::{
    AgentLogSnapshot, AgentSessionLogTracker, codex_session_log_path, provider_project_dir_name,
    session_log_path_for,
};
pub use store::{SessionEvent, SessionInfo, SessionStore, SessionUpsert, StoredMessage, unix_now};
