//! Status badge widget — coloured pill indicating agent or tool state.

/// Small coloured badge used in status bars and panel headers.
#[derive(Debug, Default)]
pub struct StatusBadge {
    /// Label text shown inside the badge.
    pub label: String,
    /// Badge variant controls the colour.
    pub variant: BadgeVariant,
}

/// Colour variant for a [`StatusBadge`].
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum BadgeVariant {
    #[default]
    Neutral,
    Success,
    Warning,
    Error,
    Info,
}

impl StatusBadge {
    /// Create a new [`StatusBadge`].
    pub fn new(label: impl Into<String>, variant: BadgeVariant) -> Self {
        Self {
            label: label.into(),
            variant,
        }
    }
}
