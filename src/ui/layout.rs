//! Terminal layout — splits the screen into named panel areas.
//!
//! The layout follows a "bottom-up" philosophy similar to Claude Code:
//! the conversation occupies most of the vertical space, the input box sits
//! just above the status bar, and the status bar is always visible at the
//! very bottom of the screen.
//!
//! ```text
//! ┌──────────────────────────────────────────────────────┐
//! │                                                      │
//! │                  conversation / chat                 │  Min(5)
//! │                                                      │
//! ├──────────────────────────────────────────────────────┤
//! │  ❯ _                                                 │  Length(3)
//! ├──────────────────────────────────────────────────────┤
//! │  llama3 │ Idle │ 0 tok │ session-abc                 │  Length(1)
//! └──────────────────────────────────────────────────────┘
//! ```

use ratatui::layout::{Constraint, Direction, Layout, Rect};

use crate::app::state::AppState;
use crate::ui::panels::PanelId;

// Re-export LayoutPreset so callers can import from either location.
pub use crate::app::state::LayoutPreset;

// ── PanelAreas ────────────────────────────────────────────────────────────────

/// Named screen regions produced by [`LayoutManager::build`].
#[derive(Debug, Clone, Copy, Default)]
pub struct PanelAreas {
    /// Primary conversation area (scrollable list of messages).
    pub chat: Rect,
    /// Single-line text input area with prompt prefix.
    pub input: Rect,
    /// Single-line status bar at the very bottom.
    pub status_bar: Rect,
    /// Optional right-side panel (tool output / sessions).
    pub side: Option<Rect>,
}

// ── LayoutManager ─────────────────────────────────────────────────────────────

/// Manages which panels are visible and which panel has focus.
///
/// Rules:
/// - At least one panel is always visible (guard enforced in toggle).
/// - Tab cycles forward through visible panels.
/// - Shift+Tab cycles backward.
/// - When the focused panel is hidden, focus moves to the next visible one.
#[derive(Debug, Clone)]
pub struct LayoutManager {
    /// Ordered list of *all* known panels.
    all_panels: Vec<PanelId>,
    /// Which panels are currently visible (subset of `all_panels`).
    visible: Vec<PanelId>,
    /// The panel that currently holds keyboard focus.
    focused: PanelId,
    /// The active layout preset.
    pub preset: LayoutPreset,
}

impl Default for LayoutManager {
    fn default() -> Self {
        Self::new()
    }
}

impl LayoutManager {
    /// Create a new layout manager with the default visible set (Chat only).
    pub fn new() -> Self {
        Self {
            all_panels: vec![
                PanelId::Chat,
                PanelId::ToolOutput,
                PanelId::FilePreview,
                PanelId::Sessions,
                PanelId::TokenDash,
                PanelId::AgentStatus,
            ],
            visible: vec![PanelId::Chat],
            focused: PanelId::Chat,
            preset: LayoutPreset::Wide,
        }
    }

    // ── Visibility ────────────────────────────────────────────────────────────

    /// Returns the list of currently visible panels.
    pub fn visible_panels(&self) -> &[PanelId] {
        &self.visible
    }

    /// Returns true if the given panel is currently visible.
    pub fn is_visible(&self, id: PanelId) -> bool {
        self.visible.contains(&id)
    }

    /// Toggle visibility of a panel.
    ///
    /// If hiding would leave zero visible panels, the request is ignored
    /// (at-least-one guard).
    /// If hiding the focused panel, focus moves to the next visible one.
    pub fn toggle_panel(&mut self, id: PanelId) {
        if self.is_visible(id) {
            // Guard: do not hide the last panel.
            if self.visible.len() == 1 {
                return;
            }
            self.visible.retain(|&p| p != id);

            // If we just hid the focused panel, move focus.
            if self.focused == id {
                self.focused = self.visible[0];
            }
        } else {
            // Show the panel — insert it in canonical order.
            self.show_panel(id);
        }
    }

    /// Show a panel (add to visible list in canonical order).
    pub fn show_panel(&mut self, id: PanelId) {
        if !self.is_visible(id) {
            // Insert in canonical order based on all_panels.
            let pos = self
                .all_panels
                .iter()
                .position(|&p| p == id)
                .unwrap_or(usize::MAX);
            let insert_at = self
                .visible
                .iter()
                .position(|&v| {
                    self.all_panels
                        .iter()
                        .position(|&p| p == v)
                        .unwrap_or(usize::MAX)
                        > pos
                })
                .unwrap_or(self.visible.len());
            self.visible.insert(insert_at, id);
        }
    }

    // ── Focus ─────────────────────────────────────────────────────────────────

    /// Returns the currently focused panel.
    pub fn focused(&self) -> PanelId {
        self.focused
    }

    /// Move focus to the next visible panel (Tab).
    pub fn focus_next(&mut self) {
        if self.visible.is_empty() {
            return;
        }
        let idx = self
            .visible
            .iter()
            .position(|&p| p == self.focused)
            .unwrap_or(0);
        self.focused = self.visible[(idx + 1) % self.visible.len()];
    }

    /// Move focus to the previous visible panel (Shift+Tab).
    pub fn focus_prev(&mut self) {
        if self.visible.is_empty() {
            return;
        }
        let idx = self
            .visible
            .iter()
            .position(|&p| p == self.focused)
            .unwrap_or(0);
        let len = self.visible.len();
        self.focused = self.visible[(idx + len - 1) % len];
    }

    /// Directly set focus to a specific panel (panel must be visible).
    pub fn set_focus(&mut self, id: PanelId) {
        if self.is_visible(id) {
            self.focused = id;
        }
    }

    // ── State sync ────────────────────────────────────────────────────────────

    /// Synchronise visible panels from [`AppState`].
    pub fn visible_panels_from_state(&mut self, state: &AppState) {
        self.visible = state.visible_panels.clone();
        self.focused = state.focused_panel;
        self.preset = state.layout_preset;
    }

    // ── Layout computation ────────────────────────────────────────────────────

    /// Compute [`PanelAreas`] given the full terminal [`Rect`] and current state.
    pub fn build(&self, area: Rect, _state: &AppState) -> PanelAreas {
        // Reserve bottom rows for input (3) and status bar (1).
        let [content_area, input, status_bar] = Layout::vertical([
            Constraint::Min(5),
            Constraint::Length(3),
            Constraint::Length(1),
        ])
        .areas(area);

        match self.preset {
            LayoutPreset::Wide | LayoutPreset::Minimal => PanelAreas {
                chat: content_area,
                input,
                status_bar,
                side: None,
            },
            LayoutPreset::Sidebar => {
                // ~70% chat / ~30% side panel
                let [chat, side] = Layout::horizontal([
                    Constraint::Percentage(70),
                    Constraint::Percentage(30),
                ])
                .direction(Direction::Horizontal)
                .areas(content_area);

                PanelAreas {
                    chat,
                    input,
                    status_bar,
                    side: Some(side),
                }
            }
        }
    }
}

// ── Legacy build_layout shim ──────────────────────────────────────────────────

/// Compute [`PanelAreas`] given the full terminal [`Rect`] and current state.
///
/// Retained for backward compatibility with callers that haven't yet been
/// migrated to [`LayoutManager`].
pub fn build_layout(area: Rect, _state: &AppState) -> PanelAreas {
    let [chat, input, status_bar] = Layout::vertical([
        Constraint::Min(5),
        Constraint::Length(3),
        Constraint::Length(1),
    ])
    .areas(area);

    PanelAreas {
        chat,
        input,
        status_bar,
        side: None,
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_manager_with(panels: Vec<PanelId>) -> LayoutManager {
        let mut mgr = LayoutManager::new();
        mgr.visible = panels.clone();
        mgr.focused = panels[0];
        mgr
    }

    // ── Focus ring ────────────────────────────────────────────────────────────

    /// Tab moves focus to the next visible panel.
    #[test]
    fn test_focus_ring_cycles_through_visible_panels() {
        let mut mgr = make_manager_with(vec![
            PanelId::Chat,
            PanelId::ToolOutput,
            PanelId::Sessions,
        ]);
        assert_eq!(mgr.focused(), PanelId::Chat);

        mgr.focus_next();
        assert_eq!(mgr.focused(), PanelId::ToolOutput);

        mgr.focus_next();
        assert_eq!(mgr.focused(), PanelId::Sessions);
    }

    /// Tab from the last panel wraps around to the first.
    #[test]
    fn test_focus_ring_wraps_around() {
        let mut mgr = make_manager_with(vec![
            PanelId::Chat,
            PanelId::ToolOutput,
            PanelId::Sessions,
        ]);
        // Advance to last
        mgr.focus_next();
        mgr.focus_next();
        assert_eq!(mgr.focused(), PanelId::Sessions);

        // Wrap
        mgr.focus_next();
        assert_eq!(mgr.focused(), PanelId::Chat);
    }

    /// Shift+Tab goes backward through the focus ring.
    #[test]
    fn test_shift_tab_reverses() {
        let mut mgr = make_manager_with(vec![
            PanelId::Chat,
            PanelId::ToolOutput,
            PanelId::Sessions,
        ]);
        // Start at Chat — going backward wraps to Sessions.
        mgr.focus_prev();
        assert_eq!(mgr.focused(), PanelId::Sessions);

        mgr.focus_prev();
        assert_eq!(mgr.focused(), PanelId::ToolOutput);

        mgr.focus_prev();
        assert_eq!(mgr.focused(), PanelId::Chat);
    }

    // ── Panel toggling ────────────────────────────────────────────────────────

    /// Toggling a hidden panel makes it visible.
    #[test]
    fn test_toggle_panel_visibility() {
        let mut mgr = LayoutManager::new();
        // Start: only Chat is visible.
        assert!(!mgr.is_visible(PanelId::ToolOutput));

        mgr.toggle_panel(PanelId::ToolOutput);
        assert!(mgr.is_visible(PanelId::ToolOutput));

        // Toggle again — hides it.
        mgr.toggle_panel(PanelId::ToolOutput);
        assert!(!mgr.is_visible(PanelId::ToolOutput));
    }

    /// The guard prevents hiding the only visible panel.
    #[test]
    fn test_cannot_hide_last_panel() {
        let mut mgr = LayoutManager::new();
        // Only Chat is visible.
        assert_eq!(mgr.visible_panels().len(), 1);

        mgr.toggle_panel(PanelId::Chat);
        // Chat should still be visible.
        assert!(mgr.is_visible(PanelId::Chat));
        assert_eq!(mgr.visible_panels().len(), 1);
    }

    /// Hiding the focused panel moves focus to the next visible one.
    #[test]
    fn test_focus_moves_on_hide() {
        let mut mgr = make_manager_with(vec![PanelId::Chat, PanelId::ToolOutput]);
        mgr.set_focus(PanelId::ToolOutput);
        assert_eq!(mgr.focused(), PanelId::ToolOutput);

        // Hide the focused panel.
        mgr.toggle_panel(PanelId::ToolOutput);

        // Focus must not still be on ToolOutput.
        assert_ne!(mgr.focused(), PanelId::ToolOutput);
        // Must now be on a visible panel.
        assert!(mgr.is_visible(mgr.focused()));
    }

    // ── Layout presets ────────────────────────────────────────────────────────

    /// Wide preset gives the full content area to chat with no side panel.
    #[test]
    fn test_layout_preset_wide() {
        let mut mgr = LayoutManager::new();
        mgr.preset = LayoutPreset::Wide;
        let state = AppState::default();
        let area = Rect::new(0, 0, 200, 50);
        let areas = mgr.build(area, &state);

        assert_eq!(areas.chat.width, 200);
        assert!(areas.side.is_none());
    }

    /// Sidebar preset gives ~70% to chat and ~30% to the side panel.
    #[test]
    fn test_layout_preset_sidebar() {
        let mut mgr = LayoutManager::new();
        mgr.preset = LayoutPreset::Sidebar;
        let state = AppState::default();
        let area = Rect::new(0, 0, 200, 50);
        let areas = mgr.build(area, &state);

        assert!(areas.side.is_some());
        let side = areas.side.unwrap();

        // Chat should be roughly 70% of 200 = 140 cols.
        // ratatui percentage constraints might be off by ±1, allow ±5.
        let chat_pct = (areas.chat.width as f32 / 200.0 * 100.0) as u16;
        let side_pct = (side.width as f32 / 200.0 * 100.0) as u16;
        assert!(chat_pct >= 65 && chat_pct <= 75, "chat_pct={}", chat_pct);
        assert!(side_pct >= 25 && side_pct <= 35, "side_pct={}", side_pct);

        // Combined width must equal total.
        assert_eq!(areas.chat.width + side.width, 200);
    }
}
