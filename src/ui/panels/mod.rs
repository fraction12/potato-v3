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
///
/// Note: `Custom(String)` prevents this type from being `Copy`; use `.clone()`
/// where a copy was previously implicit.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum PanelId {
    Chat,
    ToolOutput,
    FilePreview,
    Sessions,
    TokenDash,
    AgentStatus,
    /// Dynamically-named extension panel.
    Custom(String),
}

// ── PanelAction ───────────────────────────────────────────────────────────────

/// Actions that a panel can emit after handling a key event.
#[derive(Debug, Clone, PartialEq)]
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
    /// Send text to the active agent (compose path for input panels).
    SendToAgent(String),
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

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    // ── PanelId equality ──────────────────────────────────────────────────────

    #[test]
    fn panel_id_equality_same_variant() {
        assert_eq!(PanelId::Chat, PanelId::Chat);
        assert_eq!(PanelId::ToolOutput, PanelId::ToolOutput);
        assert_eq!(PanelId::FilePreview, PanelId::FilePreview);
        assert_eq!(PanelId::Sessions, PanelId::Sessions);
        assert_eq!(PanelId::TokenDash, PanelId::TokenDash);
        assert_eq!(PanelId::AgentStatus, PanelId::AgentStatus);
    }

    #[test]
    fn panel_id_custom_equality() {
        assert_eq!(PanelId::Custom("x".to_string()), PanelId::Custom("x".to_string()));
        assert_ne!(PanelId::Custom("x".to_string()), PanelId::Custom("y".to_string()));
    }

    #[test]
    fn panel_id_inequality_across_variants() {
        assert_ne!(PanelId::Chat, PanelId::ToolOutput);
        assert_ne!(PanelId::Chat, PanelId::Custom("Chat".to_string()));
    }

    #[test]
    fn panel_id_clone_is_equal() {
        let id = PanelId::Custom("hello".to_string());
        assert_eq!(id, id.clone());
    }

    // ── PanelId hash (usable as HashMap key) ─────────────────────────────────

    #[test]
    fn panel_id_hashable_in_set() {
        let mut set: HashSet<PanelId> = HashSet::new();
        set.insert(PanelId::Chat);
        set.insert(PanelId::ToolOutput);
        set.insert(PanelId::Chat); // duplicate
        assert_eq!(set.len(), 2);
    }

    #[test]
    fn panel_id_custom_hashable() {
        let mut set: HashSet<PanelId> = HashSet::new();
        set.insert(PanelId::Custom("a".to_string()));
        set.insert(PanelId::Custom("b".to_string()));
        set.insert(PanelId::Custom("a".to_string())); // duplicate
        assert_eq!(set.len(), 2);
    }

    // ── PanelId debug ─────────────────────────────────────────────────────────

    #[test]
    fn panel_id_debug_format() {
        assert_eq!(format!("{:?}", PanelId::Chat), "Chat");
        assert_eq!(format!("{:?}", PanelId::ToolOutput), "ToolOutput");
        assert_eq!(format!("{:?}", PanelId::Custom("foo".to_string())), r#"Custom("foo")"#);
    }

    // ── PanelAction equality ──────────────────────────────────────────────────

    #[test]
    fn panel_action_none_equality() {
        assert_eq!(PanelAction::None, PanelAction::None);
    }

    #[test]
    fn panel_action_quit_equality() {
        assert_eq!(PanelAction::Quit, PanelAction::Quit);
    }

    #[test]
    fn panel_action_request_focus_equality() {
        let a = PanelAction::RequestFocus(PanelId::Chat);
        let b = PanelAction::RequestFocus(PanelId::Chat);
        assert_eq!(a, b);
    }

    #[test]
    fn panel_action_request_focus_inequality() {
        let a = PanelAction::RequestFocus(PanelId::Chat);
        let b = PanelAction::RequestFocus(PanelId::ToolOutput);
        assert_ne!(a, b);
    }

    #[test]
    fn panel_action_toggle_panel_equality() {
        let a = PanelAction::TogglePanel(PanelId::TokenDash);
        let b = PanelAction::TogglePanel(PanelId::TokenDash);
        assert_eq!(a, b);
    }

    #[test]
    fn panel_action_send_to_agent_equality() {
        let a = PanelAction::SendToAgent("hello".to_string());
        let b = PanelAction::SendToAgent("hello".to_string());
        assert_eq!(a, b);
    }

    #[test]
    fn panel_action_send_to_agent_inequality() {
        let a = PanelAction::SendToAgent("hello".to_string());
        let b = PanelAction::SendToAgent("world".to_string());
        assert_ne!(a, b);
    }

    #[test]
    fn panel_action_send_message_vs_send_to_agent_differ() {
        let a = PanelAction::SendMessage("hello".to_string());
        let b = PanelAction::SendToAgent("hello".to_string());
        assert_ne!(a, b);
    }

    // ── PanelAction debug ─────────────────────────────────────────────────────

    #[test]
    fn panel_action_debug_format() {
        assert_eq!(format!("{:?}", PanelAction::None), "None");
        assert_eq!(format!("{:?}", PanelAction::Quit), "Quit");
        assert!(format!("{:?}", PanelAction::SendToAgent("hi".to_string())).contains("hi"));
    }

    // ── PanelAction clone ─────────────────────────────────────────────────────

    #[test]
    fn panel_action_clone() {
        let a = PanelAction::RequestFocus(PanelId::Custom("x".to_string()));
        let b = a.clone();
        assert_eq!(a, b);
    }
}
