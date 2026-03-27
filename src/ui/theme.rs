//! Earth-tone color palette and theme configuration for Potato.

use ratatui::style::{Color, Modifier, Style};

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

// WCAG-safe foreground variants for text/UI chrome on dark surfaces.
pub const BRASS: Color = Color::Rgb(171, 145, 77);
pub const STONE: Color = Color::Rgb(171, 144, 118);
pub const ROSE: Color = Color::Rgb(220, 119, 110);

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
            border: BRASS,
            border_focused: AMBER,
            user_bubble: SOIL,
            assistant_bubble: BROWN,
            tool_highlight: TAN,
            error: ROSE,
            warning: AMBER,
            success: SPROUT,
            muted: STONE,
        }
    }
}

// ── Style helpers ────────────────────────────────────────────────────────────

impl Theme {
    /// Base style: cream text on the dark background.
    pub fn base(&self) -> Style {
        Style::default().fg(self.foreground).bg(self.background)
    }

    /// Style for the user message prefix and text.
    pub fn user_message(&self) -> Style {
        Style::default().fg(CREAM)
    }

    /// Style for the assistant message prefix and text.
    pub fn assistant_message(&self) -> Style {
        Style::default().fg(TAN)
    }

    /// Style for system / informational messages.
    pub fn system_message(&self) -> Style {
        Style::default().fg(BRASS)
    }

    /// Style for error messages.
    pub fn error_message(&self) -> Style {
        Style::default().fg(ROSE)
    }

    /// Style for the ❯ input prompt prefix.
    pub fn input_prompt(&self) -> Style {
        Style::default().fg(AMBER).add_modifier(Modifier::BOLD)
    }

    /// Style for active (focused) input text.
    pub fn input_active(&self) -> Style {
        Style::default().fg(CREAM)
    }

    /// Style for disabled input text (agent is busy).
    pub fn input_disabled(&self) -> Style {
        Style::default().fg(STONE)
    }

    /// Style for the status bar background.
    pub fn status_bar(&self) -> Style {
        Style::default().fg(TAN).bg(CHARCOAL)
    }

    /// Style for the │ separators in the status bar.
    pub fn status_separator(&self) -> Style {
        Style::default().fg(BRASS).bg(CHARCOAL)
    }

    /// Border style for a tool card that is running.
    pub fn tool_running(&self) -> Style {
        Style::default().fg(AMBER)
    }

    /// Border style for a tool card that finished successfully.
    pub fn tool_done(&self) -> Style {
        Style::default().fg(SPROUT)
    }

    /// Border style for a tool card that failed.
    pub fn tool_failed(&self) -> Style {
        Style::default().fg(ROSE)
    }

    /// Style for bold inline markdown text (`**bold**`).
    pub fn bold(&self) -> Style {
        Style::default().add_modifier(Modifier::BOLD)
    }

    /// Style for inline code snippets (`code`).
    pub fn inline_code(&self) -> Style {
        Style::default().fg(TAN).bg(CHARCOAL)
    }

    /// Style for a section divider line between conversation turns.
    pub fn divider(&self) -> Style {
        Style::default().fg(BRASS)
    }

    /// Style for the approval bar header.
    pub fn approval_header(&self) -> Style {
        Style::default().fg(BG).bg(AMBER).add_modifier(Modifier::BOLD)
    }

    /// Style for approval bar body text.
    pub fn approval_body(&self) -> Style {
        Style::default().fg(CREAM).bg(CHARCOAL)
    }

    /// Muted / de-emphasised style for timestamps and secondary info.
    pub fn muted(&self) -> Style {
        Style::default().fg(STONE)
    }
}
