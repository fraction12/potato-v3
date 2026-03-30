//! Token dashboard panel — live token usage metrics and sparkline.

use crossterm::event::KeyEvent;
use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Widget},
};

use crate::app::state::AppState;
use crate::ui::theme::{AMBER, BG, CHARCOAL, CREAM, SPROUT, TAN};
use crate::ui::widgets::sparkline::TokenSparkline;
use super::{Panel, PanelAction, PanelId};

// ── Constants ─────────────────────────────────────────────────────────────────

/// Approximate cost per 1k tokens in USD (rough estimate for display only).
const COST_PER_1K: f64 = 0.000_2;

/// Max turns to keep in the sparkline history.
const SPARKLINE_CAPACITY: usize = 32;

// ── Per-model breakdown ───────────────────────────────────────────────────────

/// Token counts for a single model.
#[derive(Debug, Clone, Default)]
pub struct ModelTokens {
    /// Model identifier (e.g. `"llama3"`).
    pub model: String,
    /// Cumulative prompt tokens for this model.
    pub prompt: u64,
    /// Cumulative completion tokens for this model.
    pub completion: u64,
}

impl ModelTokens {
    /// Create a new zero-count record for a model.
    pub fn new(model: impl Into<String>) -> Self {
        Self {
            model: model.into(),
            prompt: 0,
            completion: 0,
        }
    }

    /// Total tokens (prompt + completion).
    #[must_use]
    pub fn total(&self) -> u64 {
        self.prompt + self.completion
    }
}

// ── TokenDashPanel ────────────────────────────────────────────────────────────

/// Compact strip showing prompt tokens, completion tokens, cost estimate,
/// and a sparkline of per-turn token counts.
#[derive(Debug)]
pub struct TokenDashPanel {
    /// Total cumulative prompt tokens for the session.
    pub prompt_tokens: u64,
    /// Total cumulative completion tokens for the session.
    pub completion_tokens: u64,
    /// Per-turn token counts (newest last) — drives the sparkline.
    pub turn_history: TokenSparkline,
    /// Per-model breakdown (populated when multiple models are used).
    pub model_breakdown: Vec<ModelTokens>,
    /// Whether this panel is visible.
    visible: bool,
}

impl Default for TokenDashPanel {
    fn default() -> Self {
        Self {
            prompt_tokens: 0,
            completion_tokens: 0,
            turn_history: TokenSparkline::new(SPARKLINE_CAPACITY),
            model_breakdown: Vec::new(),
            visible: true,
        }
    }
}

impl TokenDashPanel {
    /// Create a new, empty token dashboard.
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a completed agent response turn.
    ///
    /// `model` is the model that produced the response. `prompt` and
    /// `completion` are the token counts for this turn.
    pub fn record_turn(&mut self, model: &str, prompt: u64, completion: u64) {
        self.prompt_tokens += prompt;
        self.completion_tokens += completion;

        let turn_total = prompt + completion;
        self.turn_history.push(turn_total);

        // Update per-model breakdown.
        if let Some(entry) = self.model_breakdown.iter_mut().find(|m| m.model == model) {
            entry.prompt += prompt;
            entry.completion += completion;
        } else {
            let mut entry = ModelTokens::new(model);
            entry.prompt = prompt;
            entry.completion = completion;
            self.model_breakdown.push(entry);
        }
    }

    /// Total tokens this session (prompt + completion).
    pub fn total_tokens(&self) -> u64 {
        self.prompt_tokens + self.completion_tokens
    }

    /// Estimated cost in USD based on total tokens (rough approximation).
    pub fn estimated_cost(&self) -> f64 {
        self.total_tokens() as f64 / 1000.0 * COST_PER_1K
    }

    /// Build the sparkline string from the turn history.
    pub fn sparkline_str(&self) -> String {
        self.turn_history.render_str()
    }

    /// Whether the session is under the soft token budget (100k).
    pub fn is_under_budget(&self) -> bool {
        self.total_tokens() < 100_000
    }
}

impl Panel for TokenDashPanel {
    fn id(&self) -> PanelId {
        PanelId::TokenDash
    }

    fn title(&self) -> &str {
        "Token Dashboard"
    }

    fn render(&self, frame: &mut Frame, area: Rect, focused: bool, _state: &AppState) {
        let border_style = if focused {
            Style::default().fg(AMBER)
        } else {
            Style::default().fg(CHARCOAL)
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(border_style)
            .title(Span::styled(" Tokens ", Style::default().fg(TAN)))
            .style(Style::default().bg(BG));

        let inner = block.inner(area);
        frame.render_widget(block, area);

        // Value colour: Sprout when under-budget, Amber otherwise.
        let value_style = if self.is_under_budget() {
            Style::default().fg(SPROUT)
        } else {
            Style::default().fg(AMBER)
        };

        let label = Style::default().fg(CREAM);

        // Line 1: Session total
        let total_line = Line::from(vec![
            Span::styled("Total: ", label),
            Span::styled(format!("{}", self.total_tokens()), value_style),
            Span::raw("  "),
            Span::styled("Prompt: ", label),
            Span::styled(format!("{}", self.prompt_tokens), value_style),
            Span::raw("  "),
            Span::styled("Completion: ", label),
            Span::styled(format!("{}", self.completion_tokens), value_style),
            Span::raw("  "),
            Span::styled("~$", label),
            Span::styled(format!("{:.4}", self.estimated_cost()), value_style),
        ]);

        // Line 2: Sparkline
        let spark = self.sparkline_str();
        let spark_line = if spark.is_empty() {
            Line::from(Span::styled("No turns yet.", Style::default().fg(CHARCOAL)))
        } else {
            Line::from(vec![
                Span::styled("Turns: ", label),
                Span::styled(spark, value_style),
            ])
        };

        // Line 3+: Per-model breakdown (only when >1 model used)
        let mut lines = vec![total_line, spark_line];

        if self.model_breakdown.len() > 1 {
            for m in &self.model_breakdown {
                lines.push(Line::from(vec![
                    Span::styled(format!("  {} ", m.model), label),
                    Span::styled(format!("p:{} c:{}", m.prompt, m.completion), value_style),
                ]));
            }
        }

        // Trim to the available height.
        let visible: Vec<Line<'_>> = lines
            .into_iter()
            .take(inner.height as usize)
            .collect();

        let para = Paragraph::new(visible).style(Style::default().bg(BG));
        para.render(inner, frame.buffer_mut());
    }

    fn handle_key(&mut self, _key: KeyEvent, _state: &mut AppState) -> PanelAction {
        PanelAction::None
    }

    fn is_visible(&self) -> bool {
        self.visible
    }

    fn set_visible(&mut self, visible: bool) {
        self.visible = visible;
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_token_dash_initial_state() {
        let panel = TokenDashPanel::new();
        assert_eq!(panel.prompt_tokens, 0);
        assert_eq!(panel.completion_tokens, 0);
        assert_eq!(panel.total_tokens(), 0);
        assert!(panel.model_breakdown.is_empty());
        assert!(panel.turn_history.data.is_empty());
    }

    #[test]
    fn test_record_turn_accumulates() {
        let mut panel = TokenDashPanel::new();
        panel.record_turn("llama3", 100, 50);
        assert_eq!(panel.prompt_tokens, 100);
        assert_eq!(panel.completion_tokens, 50);
        assert_eq!(panel.total_tokens(), 150);

        panel.record_turn("llama3", 200, 80);
        assert_eq!(panel.prompt_tokens, 300);
        assert_eq!(panel.completion_tokens, 130);
        assert_eq!(panel.total_tokens(), 430);
    }

    #[test]
    fn test_sparkline_builds_from_history() {
        let mut panel = TokenDashPanel::new();
        // Push increasing values.
        panel.record_turn("m", 10, 0);
        panel.record_turn("m", 100, 0);
        panel.record_turn("m", 1000, 0);

        let spark = panel.sparkline_str();
        // Should have 3 chars, last should be tallest (█).
        assert_eq!(spark.chars().count(), 3);
        assert_eq!(spark.chars().last().unwrap(), '█');
    }

    #[test]
    fn test_sparkline_empty_when_no_turns() {
        let panel = TokenDashPanel::new();
        assert_eq!(panel.sparkline_str(), "");
    }

    #[test]
    fn test_sparkline_all_equal_values() {
        let mut panel = TokenDashPanel::new();
        panel.record_turn("m", 100, 0);
        panel.record_turn("m", 100, 0);
        panel.record_turn("m", 100, 0);

        let spark = panel.sparkline_str();
        // All equal → mid bar (delegates to TokenSparkline::render_str).
        assert!(spark.chars().all(|c| c == '▅'));
    }

    #[test]
    fn test_model_breakdown_single_model() {
        let mut panel = TokenDashPanel::new();
        panel.record_turn("llama3", 100, 50);
        panel.record_turn("llama3", 200, 75);

        assert_eq!(panel.model_breakdown.len(), 1);
        assert_eq!(panel.model_breakdown[0].prompt, 300);
        assert_eq!(panel.model_breakdown[0].completion, 125);
    }

    #[test]
    fn test_model_breakdown_multiple_models() {
        let mut panel = TokenDashPanel::new();
        panel.record_turn("llama3", 100, 50);
        panel.record_turn("gpt-4o", 200, 75);

        assert_eq!(panel.model_breakdown.len(), 2);
        let llama = panel.model_breakdown.iter().find(|m| m.model == "llama3").unwrap();
        assert_eq!(llama.total(), 150);
        let gpt = panel.model_breakdown.iter().find(|m| m.model == "gpt-4o").unwrap();
        assert_eq!(gpt.total(), 275);
    }

    #[test]
    fn test_estimated_cost_proportional() {
        let mut panel = TokenDashPanel::new();
        panel.record_turn("m", 1000, 0);
        let cost1k = panel.estimated_cost();

        panel.record_turn("m", 1000, 0);
        let cost2k = panel.estimated_cost();

        assert!((cost2k - 2.0 * cost1k).abs() < 1e-10);
    }

    #[test]
    fn test_is_under_budget_true_when_low() {
        let panel = TokenDashPanel::new(); // 0 tokens
        assert!(panel.is_under_budget());
    }

    #[test]
    fn test_is_under_budget_false_over_100k() {
        let mut panel = TokenDashPanel::new();
        panel.record_turn("m", 100_000, 0);
        assert!(!panel.is_under_budget());
    }

    #[test]
    fn test_sparkline_capacity_capped() {
        let mut panel = TokenDashPanel::new();
        // Push more than SPARKLINE_CAPACITY entries.
        for i in 0..=SPARKLINE_CAPACITY + 5 {
            panel.record_turn("m", i as u64, 0);
        }
        // Sparkline should not exceed capacity.
        assert!(panel.turn_history.data.len() <= SPARKLINE_CAPACITY);
    }

    #[test]
    fn test_panel_id() {
        let panel = TokenDashPanel::new();
        assert_eq!(panel.id(), PanelId::TokenDash);
    }

    #[test]
    fn test_panel_title() {
        let panel = TokenDashPanel::new();
        assert_eq!(panel.title(), "Token Dashboard");
    }

    #[test]
    fn test_panel_visibility() {
        let mut panel = TokenDashPanel::new();
        assert!(panel.is_visible());
        panel.set_visible(false);
        assert!(!panel.is_visible());
        panel.set_visible(true);
        assert!(panel.is_visible());
    }

    #[test]
    fn test_model_tokens_total() {
        let mut mt = ModelTokens::new("test");
        mt.prompt = 300;
        mt.completion = 100;
        assert_eq!(mt.total(), 400);
    }
}
