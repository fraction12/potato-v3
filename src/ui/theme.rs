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
    #[must_use]
    pub fn base(&self) -> Style {
        Style::default().fg(self.foreground).bg(self.background)
    }

    /// Style for the user message prefix and text.
    #[must_use]
    pub fn user_message(&self) -> Style {
        Style::default().fg(CREAM)
    }

    /// Style for the assistant message prefix and text.
    #[must_use]
    pub fn assistant_message(&self) -> Style {
        Style::default().fg(TAN)
    }

    /// Style for system / informational messages.
    #[must_use]
    pub fn system_message(&self) -> Style {
        Style::default().fg(BRASS)
    }

    /// Style for error messages.
    #[must_use]
    pub fn error_message(&self) -> Style {
        Style::default().fg(ROSE)
    }

    /// Style for the ❯ input prompt prefix.
    #[must_use]
    pub fn input_prompt(&self) -> Style {
        Style::default().fg(AMBER).add_modifier(Modifier::BOLD)
    }

    /// Style for active (focused) input text.
    #[must_use]
    pub fn input_active(&self) -> Style {
        Style::default().fg(CREAM)
    }

    /// Style for disabled input text (agent is busy).
    #[must_use]
    pub fn input_disabled(&self) -> Style {
        Style::default().fg(STONE)
    }

    /// Style for the status bar background.
    #[must_use]
    pub fn status_bar(&self) -> Style {
        Style::default().fg(TAN).bg(CHARCOAL)
    }

    /// Style for the │ separators in the status bar.
    #[must_use]
    pub fn status_separator(&self) -> Style {
        Style::default().fg(BRASS).bg(CHARCOAL)
    }

    /// Border style for a tool card that is running.
    #[must_use]
    pub fn tool_running(&self) -> Style {
        Style::default().fg(AMBER)
    }

    /// Border style for a tool card that finished successfully.
    #[must_use]
    pub fn tool_done(&self) -> Style {
        Style::default().fg(SPROUT)
    }

    /// Border style for a tool card that failed.
    #[must_use]
    pub fn tool_failed(&self) -> Style {
        Style::default().fg(ROSE)
    }

    /// Style for bold inline markdown text (`**bold**`).
    #[must_use]
    pub fn bold(&self) -> Style {
        Style::default().add_modifier(Modifier::BOLD)
    }

    /// Style for inline code snippets (`code`).
    #[must_use]
    pub fn inline_code(&self) -> Style {
        Style::default().fg(TAN).bg(CHARCOAL)
    }

    /// Style for a section divider line between conversation turns.
    #[must_use]
    pub fn divider(&self) -> Style {
        Style::default().fg(BRASS)
    }

    /// Style for the approval bar header.
    #[must_use]
    pub fn approval_header(&self) -> Style {
        Style::default().fg(BG).bg(AMBER).add_modifier(Modifier::BOLD)
    }

    /// Style for approval bar body text.
    #[must_use]
    pub fn approval_body(&self) -> Style {
        Style::default().fg(CREAM).bg(CHARCOAL)
    }

    /// Muted / de-emphasised style for timestamps and secondary info.
    #[must_use]
    pub fn muted(&self) -> Style {
        Style::default().fg(STONE)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Color constants ───────────────────────────────────────────────────────

    #[test]
    fn palette_colors_are_distinct() {
        let colors = [TAN, BROWN, SOIL, SPROUT, RUST_RED, AMBER, CREAM, CHARCOAL, BG, BRASS, STONE, ROSE];
        for (i, a) in colors.iter().enumerate() {
            for (j, b) in colors.iter().enumerate() {
                if i != j {
                    assert_ne!(a, b, "palette colors at index {i} and {j} should differ");
                }
            }
        }
    }

    // ── Theme defaults ────────────────────────────────────────────────────────

    #[test]
    fn default_theme_uses_expected_background_and_foreground() {
        let theme = Theme::default();
        assert_eq!(theme.background, BG);
        assert_eq!(theme.foreground, CREAM);
    }

    #[test]
    fn default_theme_fields_are_from_palette() {
        let theme = Theme::default();
        let palette = [TAN, BROWN, SOIL, SPROUT, RUST_RED, AMBER, CREAM, CHARCOAL, BG, BRASS, STONE, ROSE];
        let theme_colors = [
            theme.background, theme.foreground, theme.border, theme.border_focused,
            theme.user_bubble, theme.assistant_bubble, theme.tool_highlight,
            theme.error, theme.warning, theme.success, theme.muted,
        ];
        for (i, c) in theme_colors.iter().enumerate() {
            assert!(palette.contains(c), "theme field at index {i} ({c:?}) is not from the palette");
        }
    }

    // ── Style helpers ─────────────────────────────────────────────────────────

    #[test]
    fn base_style_has_fg_and_bg() {
        let theme = Theme::default();
        let style = theme.base();
        assert_eq!(style.fg, Some(CREAM));
        assert_eq!(style.bg, Some(BG));
    }

    #[test]
    fn user_message_uses_cream() {
        let style = Theme::default().user_message();
        assert_eq!(style.fg, Some(CREAM));
    }

    #[test]
    fn assistant_message_uses_tan() {
        let style = Theme::default().assistant_message();
        assert_eq!(style.fg, Some(TAN));
    }

    #[test]
    fn system_message_uses_brass() {
        let style = Theme::default().system_message();
        assert_eq!(style.fg, Some(BRASS));
    }

    #[test]
    fn error_message_uses_rose() {
        let style = Theme::default().error_message();
        assert_eq!(style.fg, Some(ROSE));
    }

    #[test]
    fn input_prompt_is_bold_amber() {
        let style = Theme::default().input_prompt();
        assert_eq!(style.fg, Some(AMBER));
        assert!(style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn input_active_vs_disabled_differ() {
        let theme = Theme::default();
        assert_ne!(theme.input_active().fg, theme.input_disabled().fg);
    }

    #[test]
    fn status_bar_has_charcoal_bg() {
        let style = Theme::default().status_bar();
        assert_eq!(style.bg, Some(CHARCOAL));
        assert_eq!(style.fg, Some(TAN));
    }

    #[test]
    fn tool_states_use_distinct_colors() {
        let theme = Theme::default();
        let running = theme.tool_running().fg;
        let done = theme.tool_done().fg;
        let failed = theme.tool_failed().fg;
        assert_ne!(running, done);
        assert_ne!(done, failed);
        assert_ne!(running, failed);
    }

    #[test]
    fn bold_style_has_bold_modifier() {
        let style = Theme::default().bold();
        assert!(style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn inline_code_has_tan_on_charcoal() {
        let style = Theme::default().inline_code();
        assert_eq!(style.fg, Some(TAN));
        assert_eq!(style.bg, Some(CHARCOAL));
    }

    #[test]
    fn approval_header_is_bold_with_amber_bg() {
        let style = Theme::default().approval_header();
        assert_eq!(style.bg, Some(AMBER));
        assert_eq!(style.fg, Some(BG));
        assert!(style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn muted_style_uses_stone() {
        let style = Theme::default().muted();
        assert_eq!(style.fg, Some(STONE));
    }
}
