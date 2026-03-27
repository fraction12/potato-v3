//! Session management — persistence, history, and export.

pub mod discovery;
pub mod export;
pub mod history;
pub mod store;

pub use discovery::discover_historical_sessions;
pub use history::MessageHistory;
pub use store::{SessionEvent, SessionInfo, SessionStore, StoredMessage, unix_now};
