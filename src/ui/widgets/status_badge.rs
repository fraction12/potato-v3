//! Status badge widget — coloured inline label indicating agent or tool state.
//!
//! Badges are used inside the status bar and panel headers to show the
//! current agent phase at a glance.

use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span},
};

use crate::ui::theme::{Theme, AMBER, BG, CHARCOAL, CREAM, RUST_RED, SPROUT, TAN};

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
            BadgeVariant::Error => Style::default().fg(RUST_RED).add_modifier(Modifier::BOLD),
            BadgeVariant::Info => Style::default().fg(CREAM),
        }
    }

    /// Render the badge as a [`Span`] suitable for inline use in a [`Line`].
    pub fn to_span(&self, theme: &Theme) -> Span<'static> {
        Span::styled(self.label.clone(), self.style(theme))
    }
}

/// Build a [`StatusBadge`] from an [`AgentState`] label string.
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
