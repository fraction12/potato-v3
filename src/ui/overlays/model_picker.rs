//! Model picker overlay — select which LLM model to use.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph, Widget},
};

use super::{Overlay, OverlayAction};
use crate::ui::theme::{AMBER, BG, CHARCOAL, CREAM, SOIL, STONE};

/// Modal listing available models; pressing Enter switches the active model.
#[derive(Debug, Default)]
pub struct ModelPicker {
    /// List of available model names.
    pub models: Vec<String>,
    /// Currently highlighted model index.
    pub selected: usize,
}

impl ModelPicker {
    /// Create a new model picker with the given model list and current selection.
    pub fn new(models: Vec<String>, selected: usize) -> Self {
        let clamped = if models.is_empty() {
            0
        } else {
            selected.min(models.len() - 1)
        };
        Self {
            models,
            selected: clamped,
        }
    }

    /// Move selection up (wraps).
    pub fn select_up(&mut self) {
        if self.models.is_empty() {
            return;
        }
        if self.selected == 0 {
            self.selected = self.models.len() - 1;
        } else {
            self.selected -= 1;
        }
    }

    /// Move selection down (wraps).
    pub fn select_down(&mut self) {
        if self.models.is_empty() {
            return;
        }
        self.selected = (self.selected + 1) % self.models.len();
    }
}

impl Overlay for ModelPicker {
    fn title(&self) -> &str {
        "Model"
    }

    fn render(&self, frame: &mut Frame, area: Rect) {
        if self.models.is_empty() {
            return;
        }

        let max_rows = self.models.len().min(12) as u16;
        let height = max_rows + 2; // border top + bottom

        // Calculate width based on longest model name + indicator space.
        let max_name_len = self.models.iter().map(|m| m.len()).max().unwrap_or(10);
        let width = (max_name_len as u16 + 6).max(30).min(area.width);

        // Center horizontally and vertically.
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

        for (i, model) in self.models.iter().enumerate().take(inner.height as usize) {
            let row_area = Rect::new(inner.x, inner.y + i as u16, inner.width, 1);
            let is_selected = i == self.selected;

            let bg = if is_selected { CHARCOAL } else { BG };

            let indicator = if is_selected { "● " } else { "  " };
            let indicator_style = if is_selected {
                Style::default().fg(AMBER).bg(bg)
            } else {
                Style::default().fg(STONE).bg(bg)
            };

            let name_style = if is_selected {
                Style::default()
                    .fg(CREAM)
                    .bg(bg)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(STONE).bg(bg)
            };

            let line = Line::from(vec![
                Span::styled(indicator, indicator_style),
                Span::styled(model.as_str(), name_style),
            ]);

            Paragraph::new(line).render(row_area, frame.buffer_mut());
        }
    }

    fn handle_key(&mut self, key: KeyEvent) -> OverlayAction {
        match key.code {
            KeyCode::Esc => OverlayAction::Close,
            KeyCode::Enter => {
                if let Some(model) = self.models.get(self.selected) {
                    OverlayAction::Select(model.clone())
                } else {
                    OverlayAction::Close
                }
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.select_up();
                OverlayAction::None
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.select_down();
                OverlayAction::None
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
    fn test_model_picker_title() {
        let picker = ModelPicker::default();
        assert_eq!(picker.title(), "Model");
    }

    #[test]
    fn test_model_picker_navigation_wraps() {
        let mut picker =
            ModelPicker::new(vec!["llama3".into(), "gpt-4o".into(), "mistral".into()], 0);
        assert_eq!(picker.selected, 0);

        // Up from 0 should wrap to last.
        picker.select_up();
        assert_eq!(picker.selected, 2);

        // Down from last should wrap to 0.
        picker.select_down();
        assert_eq!(picker.selected, 0);
    }

    #[test]
    fn test_model_picker_enter_selects() {
        let mut picker = ModelPicker::new(vec!["llama3".into(), "gpt-4o".into()], 1);
        let action = picker.handle_key(key(KeyCode::Enter));
        assert_eq!(action, OverlayAction::Select("gpt-4o".into()));
    }

    #[test]
    fn test_model_picker_esc_closes() {
        let mut picker = ModelPicker::new(vec!["llama3".into()], 0);
        let action = picker.handle_key(key(KeyCode::Esc));
        assert_eq!(action, OverlayAction::Close);
    }

    #[test]
    fn test_model_picker_empty_enter_closes() {
        let mut picker = ModelPicker::default();
        let action = picker.handle_key(key(KeyCode::Enter));
        assert_eq!(action, OverlayAction::Close);
    }

    #[test]
    fn test_model_picker_clamps_selection() {
        let picker = ModelPicker::new(vec!["a".into(), "b".into()], 99);
        assert_eq!(picker.selected, 1);
    }

    #[test]
    fn test_model_picker_down_key() {
        let mut picker = ModelPicker::new(vec!["a".into(), "b".into(), "c".into()], 0);
        let action = picker.handle_key(key(KeyCode::Down));
        assert_eq!(action, OverlayAction::None);
        assert_eq!(picker.selected, 1);
    }
}
