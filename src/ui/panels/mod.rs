//! UI panels — each panel owns a region of the terminal.
//!
//! Every panel implements the [`Panel`] trait and advertises itself with a
//! [`PanelId`]. The layout manager uses these IDs to build the focus ring
//! and route key events.

pub mod agent_status;
pub mod chat;
pub mod file_preview;
pub mod sessions;
pub mod token_dash;
pub mod tool_output;

use crossterm::event::KeyEvent;
use ratatui::{Frame, layout::Rect};

use crate::app::state::AppState;

// ── PanelId ───────────────────────────────────────────────────────────────────

/// Stable identifier for every panel in the application.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PanelId {
    Chat,
    ToolOutput,
    FilePreview,
    Sessions,
    TokenDash,
    AgentStatus,
}

// ── PanelAction ───────────────────────────────────────────────────────────────

/// Actions that a panel can emit after handling a key event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PanelAction {
    /// No action needed.
    None,
    /// Ask the layout manager to move focus to the given panel.
    RequestFocus(PanelId),
    /// Toggle the visibility of a panel.
    TogglePanel(PanelId),
    /// Quit the application.
    Quit,
    /// Send a message to the AI agent.
    SendMessage(String),
}

// ── Panel trait ───────────────────────────────────────────────────────────────

/// Trait implemented by every panel in the layout.
pub trait Panel: Send {
    /// Returns the stable identifier for this panel.
    fn id(&self) -> PanelId;

    /// Human-readable title (shown in border).
    fn title(&self) -> &str;

    /// Render the panel into `area`; `focused` controls border highlight.
    fn render(&self, frame: &mut Frame, area: Rect, focused: bool, state: &AppState);

    /// Handle a key event and return an action.
    fn handle_key(&mut self, key: KeyEvent, state: &mut AppState) -> PanelAction;

    /// Whether this panel is currently visible.
    fn is_visible(&self) -> bool;

    /// Show or hide this panel.
    fn set_visible(&mut self, visible: bool);
}
