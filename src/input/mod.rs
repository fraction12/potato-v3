//! Centralized keyboard input dispatch.
//!
//! Maps (KeyEvent, Screen, Focus) → KeyAction in one place.
//! main.rs interprets the action and mutates external state.

mod dashboard;
mod panels;
mod session;
mod terminal;
mod text_input;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::app::state::{AppScreen, AppState};

/// Result of handling a key event.
pub enum KeyAction {
    /// Key consumed — continue event loop.
    Handled,
    /// Application should quit.
    Quit,
    /// Spawn agent panes from dashboard (with roles to assign after spawn).
    SpawnDashboard,
    /// Resume a saved session by ID.
    ResumeSession(String),
    /// Spawn a new agent pane (from agent picker or /new command).
    SpawnAgent,
    /// Close the active pane.
    ClosePane,
    /// Broadcast text to all panes, then schedule deferred Enter keys.
    Broadcast(String),
    /// Key not consumed — fall through to other handlers.
    Unhandled,
}

/// Top-level key dispatcher. Routes to Dashboard or Session handler.
pub fn handle_key(state: &mut AppState, key: &KeyEvent) -> KeyAction {
    match &state.screen {
        AppScreen::Dashboard(_) => dashboard::handle(state, key),
        AppScreen::Session(_) => session::handle(state, key),
    }
}
