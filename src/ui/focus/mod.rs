//! Focus ring — tracks which panel currently holds keyboard focus.
//!
//! The ring is an ordered list of [`PanelId`]s; `next()` / `prev()` cycle
//! through it with wrap-around. After the visible panel set changes (e.g. a
//! panel is toggled off), call `update_panels` to re-sync.

use crate::ui::panels::PanelId;

// ── FocusRing ─────────────────────────────────────────────────────────────────

/// Circular focus ring over an ordered set of panels.
#[derive(Debug, Clone)]
pub struct FocusRing {
    panels: Vec<PanelId>,
    focused: usize,
}

impl FocusRing {
    /// Create a new focus ring.
    ///
    /// Panics if `panels` is empty (a focus ring with no panels is undefined).
    pub fn new(panels: Vec<PanelId>) -> Self {
        assert!(!panels.is_empty(), "FocusRing requires at least one panel");
        Self { panels, focused: 0 }
    }

    /// Returns a reference to the currently focused panel id.
    pub fn focused(&self) -> &PanelId {
        &self.panels[self.focused]
    }

    /// Advance focus to the next panel (wraps from last → first).
    pub fn next(&mut self) {
        if self.panels.is_empty() {
            return;
        }
        self.focused = (self.focused + 1) % self.panels.len();
    }

    /// Move focus to the previous panel (wraps from first → last).
    pub fn prev(&mut self) {
        if self.panels.is_empty() {
            return;
        }
        let len = self.panels.len();
        self.focused = (self.focused + len - 1) % len;
    }

    /// Directly focus a panel by id.
    ///
    /// Returns `true` if the panel is in the ring and focus was set;
    /// `false` if the panel was not found (focus is unchanged).
    pub fn set(&mut self, id: &PanelId) -> bool {
        if let Some(idx) = self.panels.iter().position(|p| p == id) {
            self.focused = idx;
            true
        } else {
            false
        }
    }

    /// Re-synchronise the panel list after the visible set changes.
    ///
    /// - If the currently-focused panel is still present, focus is preserved.
    /// - Otherwise focus resets to the first panel.
    ///
    /// Panics if `panels` is empty.
    pub fn update_panels(&mut self, panels: Vec<PanelId>) {
        assert!(!panels.is_empty(), "FocusRing requires at least one panel");
        let currently_focused = self.panels.get(self.focused).cloned();
        self.panels = panels;
        // Try to restore focus to the same panel.
        if let Some(ref id) = currently_focused {
            if let Some(idx) = self.panels.iter().position(|p| p == id) {
                self.focused = idx;
                return;
            }
        }
        // Focused panel was removed — reset to first.
        self.focused = 0;
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn two_panel_ring() -> FocusRing {
        FocusRing::new(vec![PanelId::Chat, PanelId::ToolOutput])
    }

    fn three_panel_ring() -> FocusRing {
        FocusRing::new(vec![PanelId::Chat, PanelId::ToolOutput, PanelId::TokenDash])
    }

    // ── Construction ─────────────────────────────────────────────────────────

    #[test]
    fn new_ring_focuses_first_panel() {
        let ring = two_panel_ring();
        assert_eq!(ring.focused(), &PanelId::Chat);
    }

    // ── next() ────────────────────────────────────────────────────────────────

    #[test]
    fn next_advances_focus() {
        let mut ring = two_panel_ring();
        ring.next();
        assert_eq!(ring.focused(), &PanelId::ToolOutput);
    }

    #[test]
    fn next_wraps_around() {
        let mut ring = two_panel_ring();
        // Chat → ToolOutput → (wrap) → Chat
        ring.next();
        ring.next();
        assert_eq!(ring.focused(), &PanelId::Chat);
    }

    #[test]
    fn next_wraps_on_three_panel_ring() {
        let mut ring = three_panel_ring();
        ring.next(); // → ToolOutput
        ring.next(); // → TokenDash
        ring.next(); // → Chat (wrap)
        assert_eq!(ring.focused(), &PanelId::Chat);
    }

    // ── prev() ────────────────────────────────────────────────────────────────

    #[test]
    fn prev_moves_backward() {
        let mut ring = two_panel_ring();
        ring.next(); // → ToolOutput
        ring.prev(); // → Chat
        assert_eq!(ring.focused(), &PanelId::Chat);
    }

    #[test]
    fn prev_wraps_around() {
        let mut ring = two_panel_ring();
        // Start at Chat (index 0); going back should wrap to ToolOutput (last)
        ring.prev();
        assert_eq!(ring.focused(), &PanelId::ToolOutput);
    }

    #[test]
    fn prev_wraps_on_three_panel_ring() {
        let mut ring = three_panel_ring();
        ring.prev(); // Chat → TokenDash (wrap)
        assert_eq!(ring.focused(), &PanelId::TokenDash);
        ring.prev(); // TokenDash → ToolOutput
        assert_eq!(ring.focused(), &PanelId::ToolOutput);
        ring.prev(); // ToolOutput → Chat
        assert_eq!(ring.focused(), &PanelId::Chat);
    }

    // ── set() ─────────────────────────────────────────────────────────────────

    #[test]
    fn set_known_panel_succeeds() {
        let mut ring = two_panel_ring();
        let ok = ring.set(&PanelId::ToolOutput);
        assert!(ok);
        assert_eq!(ring.focused(), &PanelId::ToolOutput);
    }

    #[test]
    fn set_unknown_panel_returns_false() {
        let mut ring = two_panel_ring();
        let ok = ring.set(&PanelId::TokenDash); // not in ring
        assert!(!ok);
        // Focus must be unchanged
        assert_eq!(ring.focused(), &PanelId::Chat);
    }

    #[test]
    fn set_custom_panel_in_ring() {
        let mut ring = FocusRing::new(vec![
            PanelId::Chat,
            PanelId::Custom("sidebar".to_string()),
        ]);
        let ok = ring.set(&PanelId::Custom("sidebar".to_string()));
        assert!(ok);
        assert_eq!(ring.focused(), &PanelId::Custom("sidebar".to_string()));
    }

    // ── update_panels() ───────────────────────────────────────────────────────

    #[test]
    fn update_panels_keeps_focus_if_still_present() {
        let mut ring = three_panel_ring();
        ring.next(); // → ToolOutput
        assert_eq!(ring.focused(), &PanelId::ToolOutput);

        // Update with ToolOutput still present.
        ring.update_panels(vec![PanelId::Chat, PanelId::ToolOutput, PanelId::TokenDash]);
        assert_eq!(ring.focused(), &PanelId::ToolOutput);
    }

    #[test]
    fn update_panels_resets_to_first_if_focused_removed() {
        let mut ring = three_panel_ring();
        ring.next(); // → ToolOutput

        // Remove ToolOutput from the new list.
        ring.update_panels(vec![PanelId::Chat, PanelId::TokenDash]);
        assert_eq!(ring.focused(), &PanelId::Chat);
    }

    #[test]
    fn update_panels_single_element() {
        let mut ring = three_panel_ring();
        ring.next(); // → ToolOutput

        ring.update_panels(vec![PanelId::TokenDash]);
        assert_eq!(ring.focused(), &PanelId::TokenDash);
    }

    #[test]
    fn update_panels_preserves_focus_position_when_reordered() {
        let mut ring = FocusRing::new(vec![PanelId::Chat, PanelId::ToolOutput]);
        ring.set(&PanelId::ToolOutput);

        // Reorder — ToolOutput is now first but still present.
        ring.update_panels(vec![PanelId::ToolOutput, PanelId::Chat]);
        assert_eq!(ring.focused(), &PanelId::ToolOutput);
    }
}
