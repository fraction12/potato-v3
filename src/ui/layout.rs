//! Terminal layout: splits the screen into named panel areas.

use ratatui::layout::Rect;
#[allow(unused_imports)]
use crate::app::state::AppState;

/// Named regions for each panel in the Potato layout.
#[derive(Debug, Clone, Copy, Default)]
pub struct PanelAreas {
    /// Primary chat window.
    pub chat: Rect,
    /// Tool output / execution log.
    pub tool_output: Rect,
    /// File preview pane.
    pub file_preview: Rect,
    /// Session list sidebar.
    pub sessions: Rect,
    /// Token usage dashboard strip.
    pub token_dash: Rect,
    /// Agent status strip at the bottom.
    pub agent_status: Rect,
}

/// Compute layout areas given the full terminal [`Rect`] and current state.
///
/// Layout (stub — returns zero-sized rects until implemented):
/// ```text
/// ┌─sessions─┬──────────chat──────────┬─file_preview─┐
/// │          │                        │              │
/// │          ├─────────tool_output────┤              │
/// ├──token_dash──────────────────────────────────────┤
/// └──agent_status────────────────────────────────────┘
/// ```
pub fn build_layout(_area: Rect, _state: &AppState) -> PanelAreas {
    PanelAreas::default()
}
