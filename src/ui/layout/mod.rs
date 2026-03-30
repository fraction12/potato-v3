//! Terminal layout — splits the screen into named panel areas.
//!
//! Two co-existing layout models:
//!
//! 1. **Legacy `LegacyLayoutManager`** (the Phase-2 `build()` model) — produces
//!    named [`PanelAreas`] for the existing session screen renderer.
//!
//! 2. **`LayoutManager`** (Phase-3) — composable, preset-driven manager that
//!    returns a `HashMap<PanelId, Rect>` keyed on panel id.  This is the
//!    authoritative layout for all new panel work.
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

use std::collections::{HashMap, HashSet};

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use serde::{Deserialize, Serialize};

use crate::app::state::AppState;
use crate::ui::panels::PanelId;

// ── LayoutPreset ──────────────────────────────────────────────────────────────

/// High-level layout modes.
///
/// Re-exported from this module so callers need only import from `ui::layout`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum LayoutPreset {
    /// Chat panel 70% left, ToolOutput 30% right.
    #[default]
    Sidebar,
    /// Chat panel full width top, ToolOutput stacked below (50/50).
    Wide,
    /// Chat panel takes the entire area.
    Minimal,
}

// Re-export so callers can do `use crate::ui::layout::LayoutPreset`.
// Also keep the app::state version alive for the legacy screen.
// They are different types; existing code uses app::state::LayoutPreset.
// New panel code uses this one.

// ── LayoutManager (Phase-3) ───────────────────────────────────────────────────

/// Composable, preset-driven layout manager.
///
/// Maintains an ordered list of known panels and a visible subset.
/// `compute_areas` maps each visible panel to a terminal [`Rect`].
///
/// All logic is pure (no ratatui dependency in tests — Rect is a plain struct).
#[derive(Debug)]
pub struct LayoutManager {
    preset: LayoutPreset,
    /// Canonical ordered list of all known panels.
    panels: Vec<PanelId>,
    /// Currently visible subset (subset of `panels`).
    visible: HashSet<PanelId>,
}

impl LayoutManager {
    /// Create a new manager with the given preset.
    ///
    /// Default visible panels: `Chat` and `ToolOutput`.
    pub fn new(preset: LayoutPreset) -> Self {
        let mut visible = HashSet::new();
        visible.insert(PanelId::Chat);
        visible.insert(PanelId::ToolOutput);

        Self {
            preset,
            panels: vec![PanelId::Chat, PanelId::ToolOutput],
            visible,
        }
    }

    /// Change the active layout preset.
    pub fn set_preset(&mut self, preset: LayoutPreset) {
        self.preset = preset;
    }

    /// Returns the active preset.
    #[must_use]
    pub fn preset(&self) -> &LayoutPreset {
        &self.preset
    }

    /// Toggle a panel's visibility.
    ///
    /// If the panel is not yet in `panels`, it is appended.
    pub fn toggle_panel(&mut self, id: &PanelId) {
        if self.visible.contains(id) {
            self.visible.remove(id);
        } else {
            // Ensure panel is in the ordered list.
            if !self.panels.contains(id) {
                self.panels.push(id.clone());
            }
            self.visible.insert(id.clone());
        }
    }

    /// Returns `true` if the panel is currently visible.
    #[must_use]
    pub fn is_visible(&self, id: &PanelId) -> bool {
        self.visible.contains(id)
    }

    /// Compute terminal areas for all currently-visible panels.
    ///
    /// Returns a map from `PanelId` → `Rect`. Hidden panels are absent.
    ///
    /// Layout rules by preset:
    ///
    /// | Preset  | Description                                       |
    /// |---------|---------------------------------------------------|
    /// | Sidebar | Chat 70% left / ToolOutput 30% right (horizontal) |
    /// | Wide    | Chat top / ToolOutput bottom (vertical, 50/50)    |
    /// | Minimal | Chat takes the entire `total` rect                |
    #[must_use]
    pub fn compute_areas(&self, total: Rect) -> HashMap<PanelId, Rect> {
        let mut map = HashMap::new();

        match self.preset {
            LayoutPreset::Sidebar => {
                // Only emit areas for panels that are actually visible.
                let chat_vis = self.visible.contains(&PanelId::Chat);
                let tool_vis = self.visible.contains(&PanelId::ToolOutput);

                match (chat_vis, tool_vis) {
                    (true, true) => {
                        let [left, right] = Layout::horizontal([
                            Constraint::Percentage(70),
                            Constraint::Percentage(30),
                        ])
                        .direction(Direction::Horizontal)
                        .areas(total);
                        map.insert(PanelId::Chat, left);
                        map.insert(PanelId::ToolOutput, right);
                    }
                    (true, false) => {
                        map.insert(PanelId::Chat, total);
                    }
                    (false, true) => {
                        map.insert(PanelId::ToolOutput, total);
                    }
                    (false, false) => {}
                }
            }

            LayoutPreset::Wide => {
                let chat_vis = self.visible.contains(&PanelId::Chat);
                let tool_vis = self.visible.contains(&PanelId::ToolOutput);

                match (chat_vis, tool_vis) {
                    (true, true) => {
                        let [top, bottom] = Layout::vertical([
                            Constraint::Percentage(50),
                            Constraint::Percentage(50),
                        ])
                        .areas(total);
                        map.insert(PanelId::Chat, top);
                        map.insert(PanelId::ToolOutput, bottom);
                    }
                    (true, false) => {
                        map.insert(PanelId::Chat, total);
                    }
                    (false, true) => {
                        map.insert(PanelId::ToolOutput, total);
                    }
                    (false, false) => {}
                }
            }

            LayoutPreset::Minimal => {
                // Only Chat; everything else hidden.
                if self.visible.contains(&PanelId::Chat) {
                    map.insert(PanelId::Chat, total);
                }
                // Any other visible panels that aren't Chat get full rect too
                // (edge case: user toggled something on while in Minimal).
                for id in &self.visible {
                    if id != &PanelId::Chat {
                        map.entry(id.clone()).or_insert(total);
                    }
                }
            }
        }

        // Include any extra visible custom panels not covered above.
        // For Sidebar/Wide, only Chat+ToolOutput are given split areas;
        // any extra visible panels fall back to full rect (caller can choose).
        if matches!(self.preset, LayoutPreset::Sidebar | LayoutPreset::Wide) {
            for id in &self.visible {
                if id != &PanelId::Chat && id != &PanelId::ToolOutput {
                    map.entry(id.clone()).or_insert(total);
                }
            }
        }

        map
    }
}

// ── PanelAreas (legacy) ───────────────────────────────────────────────────────

/// Named screen regions produced by [`LegacyLayoutManager::build`].
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

// ── LegacyLayoutManager ───────────────────────────────────────────────────────

/// Legacy layout manager — retained for the existing session screen renderer.
///
/// Manages which panels are visible and which panel has focus.
///
/// Rules:
/// - At least one panel is always visible (guard enforced in toggle).
/// - Tab cycles forward through visible panels.
/// - Shift+Tab cycles backward.
/// - When the focused panel is hidden, focus moves to the next visible one.
#[derive(Debug, Clone)]
pub struct LegacyLayoutManager {
    /// Ordered list of *all* known panels.
    all_panels: Vec<PanelId>,
    /// Which panels are currently visible (subset of `all_panels`).
    visible: Vec<PanelId>,
    /// The panel that currently holds keyboard focus.
    focused: PanelId,
    /// The active layout preset (uses the legacy app::state::LayoutPreset).
    pub preset: crate::app::state::LayoutPreset,
}

impl Default for LegacyLayoutManager {
    fn default() -> Self {
        Self::new()
    }
}

impl LegacyLayoutManager {
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
            preset: crate::app::state::LayoutPreset::Wide,
        }
    }

    // ── Visibility ────────────────────────────────────────────────────────────

    /// Returns the list of currently visible panels.
    #[must_use]
    pub fn visible_panels(&self) -> &[PanelId] {
        &self.visible
    }

    /// Returns true if the given panel is currently visible.
    #[must_use]
    pub fn is_visible(&self, id: &PanelId) -> bool {
        self.visible.contains(id)
    }

    /// Toggle visibility of a panel.
    ///
    /// If hiding would leave zero visible panels, the request is ignored
    /// (at-least-one guard).
    /// If hiding the focused panel, focus moves to the next visible one.
    pub fn toggle_panel(&mut self, id: &PanelId) {
        if self.is_visible(id) {
            // Guard: do not hide the last panel.
            if self.visible.len() == 1 {
                return;
            }
            self.visible.retain(|p| p != id);

            // If we just hid the focused panel, move focus.
            if &self.focused == id {
                self.focused = self.visible[0].clone();
            }
        } else {
            // Show the panel — insert it in canonical order.
            self.show_panel(id);
        }
    }

    /// Show a panel (add to visible list in canonical order).
    pub fn show_panel(&mut self, id: &PanelId) {
        if !self.is_visible(id) {
            // Insert in canonical order based on all_panels.
            let pos = self
                .all_panels
                .iter()
                .position(|p| p == id)
                .unwrap_or(usize::MAX);
            let insert_at = self
                .visible
                .iter()
                .position(|v| {
                    self.all_panels
                        .iter()
                        .position(|p| p == v)
                        .unwrap_or(usize::MAX)
                        > pos
                })
                .unwrap_or(self.visible.len());
            self.visible.insert(insert_at, id.clone());
        }
    }

    // ── Focus ─────────────────────────────────────────────────────────────────

    /// Returns the currently focused panel.
    #[must_use]
    pub fn focused(&self) -> &PanelId {
        &self.focused
    }

    /// Move focus to the next visible panel (Tab).
    pub fn focus_next(&mut self) {
        if self.visible.is_empty() {
            return;
        }
        let idx = self
            .visible
            .iter()
            .position(|p| p == &self.focused)
            .unwrap_or(0);
        self.focused = self.visible[(idx + 1) % self.visible.len()].clone();
    }

    /// Move focus to the previous visible panel (Shift+Tab).
    pub fn focus_prev(&mut self) {
        if self.visible.is_empty() {
            return;
        }
        let idx = self
            .visible
            .iter()
            .position(|p| p == &self.focused)
            .unwrap_or(0);
        let len = self.visible.len();
        self.focused = self.visible[(idx + len - 1) % len].clone();
    }

    /// Directly set focus to a specific panel (panel must be visible).
    pub fn set_focus(&mut self, id: &PanelId) {
        if self.is_visible(id) {
            self.focused = id.clone();
        }
    }

    // ── Layout computation ────────────────────────────────────────────────────

    /// Compute [`PanelAreas`] given the full terminal [`Rect`] and current state.
    #[must_use]
    pub fn build(&self, area: Rect, _state: &AppState) -> PanelAreas {
        // Reserve bottom rows for input (3) and status bar (1).
        let [content_area, input, status_bar] = Layout::vertical([
            Constraint::Min(5),
            Constraint::Length(3),
            Constraint::Length(1),
        ])
        .areas(area);

        match self.preset {
            crate::app::state::LayoutPreset::Wide | crate::app::state::LayoutPreset::Minimal => {
                PanelAreas {
                    chat: content_area,
                    input,
                    status_bar,
                    side: None,
                }
            }
            crate::app::state::LayoutPreset::Sidebar => {
                // ~70% chat / ~30% side panel
                let [chat, side] =
                    Layout::horizontal([Constraint::Percentage(70), Constraint::Percentage(30)])
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
/// Retained for backward compatibility.
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

    // Helper: make a 100×50 rect at origin.
    fn rect(w: u16, h: u16) -> Rect {
        Rect::new(0, 0, w, h)
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Phase-3 LayoutManager tests
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn sidebar_preset_gives_two_non_overlapping_rects() {
        let mgr = LayoutManager::new(LayoutPreset::Sidebar);
        let total = rect(100, 50);
        let areas = mgr.compute_areas(total);

        let chat = areas[&PanelId::Chat];
        let tool = areas[&PanelId::ToolOutput];

        // Both present.
        assert!(areas.contains_key(&PanelId::Chat));
        assert!(areas.contains_key(&PanelId::ToolOutput));

        // Non-overlapping: one ends where the other begins.
        // Horizontal split → same y/height, different x ranges.
        assert_eq!(chat.y, tool.y);
        assert_eq!(chat.height, tool.height);
        assert_eq!(chat.x + chat.width, tool.x);
    }

    #[test]
    fn wide_preset_gives_vertically_stacked_rects() {
        let mgr = LayoutManager::new(LayoutPreset::Wide);
        let total = rect(100, 50);
        let areas = mgr.compute_areas(total);

        let chat = areas[&PanelId::Chat];
        let tool = areas[&PanelId::ToolOutput];

        // Vertically stacked → same x/width, different y ranges.
        assert_eq!(chat.x, tool.x);
        assert_eq!(chat.width, tool.width);
        assert_eq!(chat.y + chat.height, tool.y);
    }

    #[test]
    fn minimal_preset_gives_single_full_rect() {
        let mgr = LayoutManager::new(LayoutPreset::Minimal);
        let total = rect(100, 50);
        let areas = mgr.compute_areas(total);

        // Minimal only shows Chat at full size.
        assert!(areas.contains_key(&PanelId::Chat));
        assert_eq!(areas[&PanelId::Chat], total);
    }

    #[test]
    fn toggle_panel_hides_and_shows() {
        let mut mgr = LayoutManager::new(LayoutPreset::Sidebar);
        assert!(mgr.is_visible(&PanelId::Chat));

        mgr.toggle_panel(&PanelId::Chat);
        assert!(!mgr.is_visible(&PanelId::Chat));

        mgr.toggle_panel(&PanelId::Chat);
        assert!(mgr.is_visible(&PanelId::Chat));
    }

    #[test]
    fn hidden_panel_absent_from_compute_areas() {
        let mut mgr = LayoutManager::new(LayoutPreset::Sidebar);
        mgr.toggle_panel(&PanelId::ToolOutput); // hide it

        let areas = mgr.compute_areas(rect(100, 50));
        assert!(!areas.contains_key(&PanelId::ToolOutput));
        assert!(areas.contains_key(&PanelId::Chat));
    }

    #[test]
    fn areas_fill_full_terminal_width_and_height() {
        // Sidebar: both chat + tool should cover the full width.
        let mgr = LayoutManager::new(LayoutPreset::Sidebar);
        let total = rect(200, 50);
        let areas = mgr.compute_areas(total);

        let chat = areas[&PanelId::Chat];
        let tool = areas[&PanelId::ToolOutput];

        // Combined width equals total width.
        assert_eq!(chat.width + tool.width, total.width);
        // Same height.
        assert_eq!(chat.height, total.height);
        assert_eq!(tool.height, total.height);
    }

    #[test]
    fn areas_fill_full_terminal_width_and_height_wide() {
        // Wide: both chat + tool should cover the full height.
        let mgr = LayoutManager::new(LayoutPreset::Wide);
        let total = rect(100, 100);
        let areas = mgr.compute_areas(total);

        let chat = areas[&PanelId::Chat];
        let tool = areas[&PanelId::ToolOutput];

        assert_eq!(chat.height + tool.height, total.height);
        assert_eq!(chat.width, total.width);
        assert_eq!(tool.width, total.width);
    }

    #[test]
    fn set_preset_changes_layout() {
        let mut mgr = LayoutManager::new(LayoutPreset::Minimal);
        mgr.set_preset(LayoutPreset::Sidebar);
        assert_eq!(mgr.preset(), &LayoutPreset::Sidebar);
    }

    #[test]
    fn sidebar_preset_approx_70_30_split() {
        let mgr = LayoutManager::new(LayoutPreset::Sidebar);
        let total = rect(200, 50);
        let areas = mgr.compute_areas(total);

        let chat_w = areas[&PanelId::Chat].width;
        let tool_w = areas[&PanelId::ToolOutput].width;

        // 70% of 200 = 140; allow ±5 for rounding.
        assert!((135..=145).contains(&chat_w), "chat_w={}", chat_w);
        assert!((55..=65).contains(&tool_w), "tool_w={}", tool_w);
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Legacy LegacyLayoutManager tests (preserved from layout.rs)
    // ─────────────────────────────────────────────────────────────────────────

    fn make_legacy_with(panels: Vec<PanelId>) -> LegacyLayoutManager {
        let mut mgr = LegacyLayoutManager::new();
        mgr.focused = panels[0].clone();
        mgr.visible = panels;
        mgr
    }

    #[test]
    fn test_focus_ring_cycles_through_visible_panels() {
        let mut mgr = make_legacy_with(vec![PanelId::Chat, PanelId::ToolOutput, PanelId::Sessions]);
        assert_eq!(mgr.focused(), &PanelId::Chat);
        mgr.focus_next();
        assert_eq!(mgr.focused(), &PanelId::ToolOutput);
        mgr.focus_next();
        assert_eq!(mgr.focused(), &PanelId::Sessions);
    }

    #[test]
    fn test_focus_ring_wraps_around() {
        let mut mgr = make_legacy_with(vec![PanelId::Chat, PanelId::ToolOutput, PanelId::Sessions]);
        mgr.focus_next();
        mgr.focus_next();
        assert_eq!(mgr.focused(), &PanelId::Sessions);
        mgr.focus_next();
        assert_eq!(mgr.focused(), &PanelId::Chat);
    }

    #[test]
    fn test_shift_tab_reverses() {
        let mut mgr = make_legacy_with(vec![PanelId::Chat, PanelId::ToolOutput, PanelId::Sessions]);
        mgr.focus_prev();
        assert_eq!(mgr.focused(), &PanelId::Sessions);
        mgr.focus_prev();
        assert_eq!(mgr.focused(), &PanelId::ToolOutput);
        mgr.focus_prev();
        assert_eq!(mgr.focused(), &PanelId::Chat);
    }

    #[test]
    fn test_toggle_panel_visibility() {
        let mut mgr = LegacyLayoutManager::new();
        assert!(!mgr.is_visible(&PanelId::ToolOutput));
        mgr.toggle_panel(&PanelId::ToolOutput);
        assert!(mgr.is_visible(&PanelId::ToolOutput));
        mgr.toggle_panel(&PanelId::ToolOutput);
        assert!(!mgr.is_visible(&PanelId::ToolOutput));
    }

    #[test]
    fn test_cannot_hide_last_panel() {
        let mut mgr = LegacyLayoutManager::new();
        assert_eq!(mgr.visible_panels().len(), 1);
        mgr.toggle_panel(&PanelId::Chat);
        assert!(mgr.is_visible(&PanelId::Chat));
        assert_eq!(mgr.visible_panels().len(), 1);
    }

    #[test]
    fn test_focus_moves_on_hide() {
        let mut mgr = make_legacy_with(vec![PanelId::Chat, PanelId::ToolOutput]);
        mgr.set_focus(&PanelId::ToolOutput);
        assert_eq!(mgr.focused(), &PanelId::ToolOutput);
        mgr.toggle_panel(&PanelId::ToolOutput);
        assert_ne!(mgr.focused(), &PanelId::ToolOutput);
        assert!(mgr.is_visible(mgr.focused()));
    }

    #[test]
    fn test_layout_preset_wide() {
        let mut mgr = LegacyLayoutManager::new();
        mgr.preset = crate::app::state::LayoutPreset::Wide;
        let state = AppState::default();
        let area = Rect::new(0, 0, 200, 50);
        let areas = mgr.build(area, &state);
        assert_eq!(areas.chat.width, 200);
        assert!(areas.side.is_none());
    }

    #[test]
    fn test_layout_preset_sidebar() {
        let mut mgr = LegacyLayoutManager::new();
        mgr.preset = crate::app::state::LayoutPreset::Sidebar;
        let state = AppState::default();
        let area = Rect::new(0, 0, 200, 50);
        let areas = mgr.build(area, &state);
        assert!(areas.side.is_some());
        let side = areas.side.unwrap();
        let chat_pct = (areas.chat.width as f32 / 200.0 * 100.0) as u16;
        let side_pct = (side.width as f32 / 200.0 * 100.0) as u16;
        assert!((65..=75).contains(&chat_pct), "chat_pct={}", chat_pct);
        assert!((25..=35).contains(&side_pct), "side_pct={}", side_pct);
        assert_eq!(areas.chat.width + side.width, 200);
    }
}
