//! Status badge widget — coloured inline label indicating agent or tool state.
//!
//! Badges are used inside the status bar and panel headers to show the
//! current agent phase at a glance.

use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span},
};

use crate::ui::theme::{AMBER, BG, CHARCOAL, CREAM, ROSE, RUST_RED, SPROUT, TAN, Theme};

/// Colour variant for a [`StatusBadge`].
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum BadgeVariant {
    /// Neutral / informational (tan on charcoal).
    #[default]
    Neutral,
    /// Positive / success state (sprout).
    Success,
    /// Warning / pending state (amber).
    Warning,
    /// Error / failed state (rust red).
    Error,
    /// Highlighted info (cream on soil).
    Info,
}

/// Small coloured badge used in status bars and panel headers.
#[derive(Debug, Default)]
pub struct StatusBadge {
    /// Label text shown inside the badge.
    pub label: String,
    /// Badge variant controls the colour.
    pub variant: BadgeVariant,
}

impl StatusBadge {
    /// Create a new [`StatusBadge`].
    pub fn new(label: impl Into<String>, variant: BadgeVariant) -> Self {
        Self {
            label: label.into(),
            variant,
        }
    }

    /// Returns the foreground [`Style`] for this badge variant.
    pub fn style(&self, _theme: &Theme) -> Style {
        match self.variant {
            BadgeVariant::Neutral => Style::default().fg(TAN),
            BadgeVariant::Success => Style::default().fg(SPROUT),
            BadgeVariant::Warning => Style::default().fg(AMBER).add_modifier(Modifier::BOLD),
            BadgeVariant::Error => Style::default().fg(ROSE).add_modifier(Modifier::BOLD),
            BadgeVariant::Info => Style::default().fg(CREAM),
        }
    }

    /// Render the badge as a [`Span`] suitable for inline use in a [`Line`].
    pub fn to_span(&self, theme: &Theme) -> Span<'static> {
        Span::styled(self.label.clone(), self.style(theme))
    }
}

/// Build a [`StatusBadge`] from an `AgentState` label string.
impl StatusBadge {
    /// Construct a badge that reflects a given agent state label.
    pub fn from_agent_state(label: &str) -> Self {
        let variant = match label {
            "Idle" => BadgeVariant::Neutral,
            "Thinking" => BadgeVariant::Warning,
            "ToolCall" => BadgeVariant::Info,
            "Approval" => BadgeVariant::Warning,
            "Error" => BadgeVariant::Error,
            _ => BadgeVariant::Neutral,
        };
        StatusBadge::new(label.to_string(), variant)
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::Modifier;

    fn theme() -> Theme {
        Theme::default()
    }

    #[test]
    fn new_sets_label_and_variant() {
        let badge = StatusBadge::new("Running", BadgeVariant::Success);
        assert_eq!(badge.label, "Running");
        assert_eq!(badge.variant, BadgeVariant::Success);
    }

    #[test]
    fn default_is_neutral() {
        let badge = StatusBadge::default();
        assert_eq!(badge.variant, BadgeVariant::Neutral);
        assert!(badge.label.is_empty());
    }

    #[test]
    fn neutral_style_uses_tan() {
        let badge = StatusBadge::new("Idle", BadgeVariant::Neutral);
        let style = badge.style(&theme());
        assert_eq!(style.fg, Some(TAN));
        assert!(!style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn success_style_uses_sprout() {
        let badge = StatusBadge::new("Done", BadgeVariant::Success);
        let style = badge.style(&theme());
        assert_eq!(style.fg, Some(SPROUT));
    }

    #[test]
    fn warning_style_is_bold_amber() {
        let badge = StatusBadge::new("Thinking", BadgeVariant::Warning);
        let style = badge.style(&theme());
        assert_eq!(style.fg, Some(AMBER));
        assert!(style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn error_style_is_bold_rose() {
        let badge = StatusBadge::new("Error", BadgeVariant::Error);
        let style = badge.style(&theme());
        assert_eq!(style.fg, Some(ROSE));
        assert!(style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn info_style_uses_cream() {
        let badge = StatusBadge::new("ToolCall", BadgeVariant::Info);
        let style = badge.style(&theme());
        assert_eq!(style.fg, Some(CREAM));
    }

    #[test]
    fn to_span_carries_label_and_style() {
        let badge = StatusBadge::new("Ready", BadgeVariant::Success);
        let span = badge.to_span(&theme());
        assert_eq!(span.content.as_ref(), "Ready");
        assert_eq!(span.style.fg, Some(SPROUT));
    }

    #[test]
    fn from_agent_state_idle() {
        let badge = StatusBadge::from_agent_state("Idle");
        assert_eq!(badge.label, "Idle");
        assert_eq!(badge.variant, BadgeVariant::Neutral);
    }

    #[test]
    fn from_agent_state_thinking() {
        let badge = StatusBadge::from_agent_state("Thinking");
        assert_eq!(badge.variant, BadgeVariant::Warning);
    }

    #[test]
    fn from_agent_state_toolcall() {
        let badge = StatusBadge::from_agent_state("ToolCall");
        assert_eq!(badge.variant, BadgeVariant::Info);
    }

    #[test]
    fn from_agent_state_approval() {
        let badge = StatusBadge::from_agent_state("Approval");
        assert_eq!(badge.variant, BadgeVariant::Warning);
    }

    #[test]
    fn from_agent_state_error() {
        let badge = StatusBadge::from_agent_state("Error");
        assert_eq!(badge.variant, BadgeVariant::Error);
    }

    #[test]
    fn from_agent_state_unknown_falls_back_to_neutral() {
        let badge = StatusBadge::from_agent_state("SomeNewState");
        assert_eq!(badge.label, "SomeNewState");
        assert_eq!(badge.variant, BadgeVariant::Neutral);
    }

    #[test]
    fn all_variants_produce_distinct_styles() {
        let variants = [
            BadgeVariant::Neutral,
            BadgeVariant::Success,
            BadgeVariant::Warning,
            BadgeVariant::Error,
            BadgeVariant::Info,
        ];
        let styles: Vec<_> = variants
            .iter()
            .map(|v| StatusBadge::new("x", *v).style(&theme()))
            .collect();
        // Each variant should produce a unique style (fg or modifier differs)
        for i in 0..styles.len() {
            for j in (i + 1)..styles.len() {
                assert_ne!(
                    styles[i], styles[j],
                    "variants {:?} and {:?} should have distinct styles",
                    variants[i], variants[j]
                );
            }
        }
    }
}
