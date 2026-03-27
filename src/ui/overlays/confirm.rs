//! Confirmation dialog overlay — yes/no prompt for destructive actions.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    Frame,
    layout::{Alignment, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph},
};

use crate::ui::theme::{AMBER, BG, CHARCOAL, CREAM, SOIL, STONE};
use super::{Overlay, OverlayAction};

/// Compact yes/no confirmation dialog.
#[derive(Debug, Default)]
pub struct ConfirmDialog {
    /// The question to present to the user.
    pub message: String,
    /// Optional title to show in the border (falls back to "Confirm").
    pub custom_title: Option<String>,
}

impl ConfirmDialog {
    /// Create a confirmation dialog with the given message.
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            custom_title: None,
        }
    }

    /// Create a confirmation dialog with a custom title and message.
    pub fn with_title(title: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            custom_title: Some(title.into()),
        }
    }
}

impl Overlay for ConfirmDialog {
    fn title(&self) -> &str {
        self.custom_title.as_deref().unwrap_or("Confirm")
    }

    fn render(&self, frame: &mut Frame, area: Rect) {
        // Dialog is small — fixed size centered over the area.
        let msg_len = self.message.len() as u16;
        let width = (msg_len + 6).max(40).min(area.width);
        let height = 7_u16; // top border + padding + message + prompt + padding + bottom border

        let x = area.left() + area.width.saturating_sub(width) / 2;
        let y = area.top() + area.height.saturating_sub(height) / 2;

        let overlay_area = Rect::new(x, y, width, height).intersection(area);

        frame.render_widget(Clear, overlay_area);

        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(AMBER))
            .title(format!(" {} ", self.title()))
            .title_style(Style::default().fg(AMBER).add_modifier(Modifier::BOLD))
            .style(Style::default().bg(CHARCOAL));

        let inner = block.inner(overlay_area);
        frame.render_widget(block, overlay_area);

        if inner.height == 0 {
            return;
        }

        // Line 0: blank padding
        // Line 1: message (centered)
        // Line 2: blank
        // Line 3: y/n prompt (centered)
        // Line 4: blank padding

        let message_line = Line::from(Span::styled(
            self.message.as_str(),
            Style::default().fg(CREAM),
        ));

        let prompt_line = Line::from(vec![
            Span::styled("[", Style::default().fg(STONE)),
            Span::styled("y", Style::default().fg(AMBER).add_modifier(Modifier::BOLD)),
            Span::styled("] confirm   [", Style::default().fg(STONE)),
            Span::styled("n / Esc", Style::default().fg(AMBER).add_modifier(Modifier::BOLD)),
            Span::styled("] cancel", Style::default().fg(STONE)),
        ]);

        // Render centered inside inner area.
        let content = vec![
            Line::from(""),
            message_line,
            Line::from(""),
            prompt_line,
        ];

        let para = Paragraph::new(content)
            .alignment(Alignment::Center)
            .style(Style::default().bg(CHARCOAL));

        frame.render_widget(para, inner);
    }

    fn handle_key(&mut self, key: KeyEvent) -> OverlayAction {
        match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') => OverlayAction::Confirm(true),
            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                OverlayAction::Confirm(false)
            }
            _ => OverlayAction::None,
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyModifiers;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn test_confirm_title_default() {
        let d = ConfirmDialog::new("Are you sure?");
        assert_eq!(d.title(), "Confirm");
    }

    #[test]
    fn test_confirm_title_custom() {
        let d = ConfirmDialog::with_title("Delete Session", "This will be gone.");
        assert_eq!(d.title(), "Delete Session");
    }

    #[test]
    fn test_confirm_y_returns_true() {
        let mut d = ConfirmDialog::new("Delete?");
        assert_eq!(d.handle_key(key(KeyCode::Char('y'))), OverlayAction::Confirm(true));
    }

    #[test]
    fn test_confirm_capital_y_returns_true() {
        let mut d = ConfirmDialog::new("Delete?");
        assert_eq!(d.handle_key(key(KeyCode::Char('Y'))), OverlayAction::Confirm(true));
    }

    #[test]
    fn test_confirm_n_returns_false() {
        let mut d = ConfirmDialog::new("Delete?");
        assert_eq!(d.handle_key(key(KeyCode::Char('n'))), OverlayAction::Confirm(false));
    }

    #[test]
    fn test_confirm_esc_returns_false() {
        let mut d = ConfirmDialog::new("Delete?");
        assert_eq!(d.handle_key(key(KeyCode::Esc)), OverlayAction::Confirm(false));
    }

    #[test]
    fn test_confirm_other_key_is_none() {
        let mut d = ConfirmDialog::new("Delete?");
        assert_eq!(d.handle_key(key(KeyCode::Char('x'))), OverlayAction::None);
    }
}
