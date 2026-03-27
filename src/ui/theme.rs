//! Earth-tone color palette and theme configuration for Potato.

use ratatui::style::Color;

// ── Earth-tone palette ──────────────────────────────────────────────────────

pub const TAN: Color = Color::Rgb(212, 165, 116);
pub const BROWN: Color = Color::Rgb(139, 105, 20);
pub const SOIL: Color = Color::Rgb(107, 66, 38);
pub const SPROUT: Color = Color::Rgb(124, 179, 66);
pub const RUST_RED: Color = Color::Rgb(198, 40, 40);
pub const AMBER: Color = Color::Rgb(249, 168, 37);
pub const CREAM: Color = Color::Rgb(255, 248, 225);
pub const CHARCOAL: Color = Color::Rgb(62, 39, 35);
pub const BG: Color = Color::Rgb(30, 20, 15);

// ── Theme struct ────────────────────────────────────────────────────────────

/// Central theme configuration holding color assignments for UI elements.
#[derive(Debug, Clone)]
pub struct Theme {
    pub background: Color,
    pub foreground: Color,
    pub border: Color,
    pub border_focused: Color,
    pub user_bubble: Color,
    pub assistant_bubble: Color,
    pub tool_highlight: Color,
    pub error: Color,
    pub warning: Color,
    pub success: Color,
    pub muted: Color,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            background: BG,
            foreground: CREAM,
            border: CHARCOAL,
            border_focused: AMBER,
            user_bubble: SOIL,
            assistant_bubble: BROWN,
            tool_highlight: TAN,
            error: RUST_RED,
            warning: AMBER,
            success: SPROUT,
            muted: CHARCOAL,
        }
    }
}
