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

use ratatui::layout::{Constraint, Layout, Rect};

use crate::app::state::AppState;

/// Named screen regions produced by [`build_layout`].
#[derive(Debug, Clone, Copy, Default)]
pub struct PanelAreas {
    /// Primary conversation area (scrollable list of messages).
    pub chat: Rect,
    /// Single-line text input area with prompt prefix.
    pub input: Rect,
    /// Single-line status bar at the very bottom.
    pub status_bar: Rect,
}

/// Compute [`PanelAreas`] given the full terminal [`Rect`] and current state.
///
/// The `_state` parameter is available for future dynamic layout decisions
/// (e.g. showing/hiding panels based on agent phase).
pub fn build_layout(area: Rect, _state: &AppState) -> PanelAreas {
    // Three vertical slices, bottom-up:
    //   1. Chat      — fills all remaining space (Min 5 lines)
    //   2. Input box — always exactly 3 lines (border + content + border)
    //   3. Status    — exactly 1 line
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
    }
}
