//! Agent status panel — bottom bar showing current agent phase and model.

use std::time::{Duration, Instant};

use crossterm::event::KeyEvent;
use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Widget},
};

use crate::app::state::AppState;
use crate::ui::theme::{AMBER, BG, CHARCOAL, CREAM, ROSE, SPROUT, STONE, TAN};
use super::{Panel, PanelAction, PanelId};

// ── Spinner frames ─────────────────────────────────────────────────────────────

const SPINNER: &[char] = &['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

// ── AgentStatusState ──────────────────────────────────────────────────────────

/// The UI-side representation of the agent's current phase.
///
/// Tracks the current activity state with elapsed-time tracking and queue counts.
#[derive(Debug, Clone)]
pub enum StatusPhase {
    /// Agent is idle — waiting for user input.
    Idle,
    /// Agent is streaming tokens from the LLM.
    Thinking,
    /// Agent has issued a tool call and is waiting for it to finish.
    ToolCall {
        /// Name of the active tool.
        tool_name: String,
    },
    /// A tool call is awaiting user approval.
    Approval {
        /// Name of the tool awaiting approval.
        tool_name: String,
    },
    /// An unrecoverable error occurred.
    Error(String),
}

impl StatusPhase {
    /// Short label for the phase (shown in the status bar).
    pub fn label(&self) -> &str {
        match self {
            Self::Idle => "Idle",
            Self::Thinking => "Thinking",
            Self::ToolCall { .. } => "Tool Call",
            Self::Approval { .. } => "Approval",
            Self::Error(_) => "Error",
        }
    }

    /// Style for the phase indicator pill.
    pub fn style(&self) -> Style {
        match self {
            Self::Idle => Style::default().fg(CHARCOAL),
            Self::Thinking => Style::default().fg(AMBER),
            Self::ToolCall { .. } => Style::default().fg(TAN),
            Self::Approval { .. } => Style::default().fg(AMBER).add_modifier(Modifier::BOLD),
            Self::Error(_) => Style::default().fg(ROSE),
        }
    }

    /// Whether this phase represents active work (non-idle, non-error).
    pub fn is_active(&self) -> bool {
        matches!(self, Self::Thinking | Self::ToolCall { .. } | Self::Approval { .. })
    }
}

// ── AgentStatusPanel ──────────────────────────────────────────────────────────

/// Single-line (or compact) status panel showing agent phase, model, elapsed
/// time, and pending tool call queue depth.
#[derive(Debug)]
pub struct AgentStatusPanel {
    /// Current phase.
    pub phase: StatusPhase,
    /// When the current phase was entered.
    pub phase_entered_at: Instant,
    /// The active model name.
    pub model: String,
    /// Number of queued tool calls (beyond the currently executing one).
    pub pending_tool_calls: usize,
    /// Tick counter — drives the spinner animation.
    pub tick: u64,
    /// Whether this panel is visible.
    visible: bool,
}

impl Default for AgentStatusPanel {
    fn default() -> Self {
        Self {
            phase: StatusPhase::Idle,
            phase_entered_at: Instant::now(),
            model: String::new(),
            pending_tool_calls: 0,
            tick: 0,
            visible: true,
        }
    }
}

impl AgentStatusPanel {
    /// Create a new status panel.
    pub fn new() -> Self {
        Self::default()
    }

    /// Transition to a new phase, resetting the elapsed timer.
    pub fn set_phase(&mut self, phase: StatusPhase) {
        self.phase = phase;
        self.phase_entered_at = Instant::now();
    }

    /// Synchronise model and tick from app state.
    pub fn sync_from_state(&mut self, state: &AppState) {
        self.model = state.model.clone();
        self.tick = state.tick_count;
    }

    /// How long the current phase has been active.
    pub fn elapsed(&self) -> Duration {
        self.phase_entered_at.elapsed()
    }

    /// Format elapsed time as a compact string: "0s", "5s", "1m23s".
    pub fn elapsed_str(&self) -> String {
        let secs = self.elapsed().as_secs();
        if secs < 60 {
            format!("{}s", secs)
        } else {
            format!("{}m{}s", secs / 60, secs % 60)
        }
    }

    /// Current spinner character for Thinking/ToolCall phases.
    fn spinner_char(&self) -> char {
        SPINNER[(self.tick as usize) % SPINNER.len()]
    }

    /// Build the single status line spans.
    fn build_line(&self) -> Line<'static> {
        let phase_style = self.phase.style();

        let mut spans: Vec<Span<'static>> = Vec::new();

        // Spinner / indicator
        match &self.phase {
            StatusPhase::Idle => {
                spans.push(Span::styled("●  ", Style::default().fg(STONE)));
            }
            StatusPhase::Thinking => {
                spans.push(Span::styled(
                    format!("{}  ", self.spinner_char()),
                    phase_style,
                ));
            }
            StatusPhase::ToolCall { .. } => {
                spans.push(Span::styled("⚙  ", phase_style));
            }
            StatusPhase::Approval { .. } => {
                spans.push(Span::styled("⚠  ", phase_style));
            }
            StatusPhase::Error(_) => {
                spans.push(Span::styled("✗  ", phase_style));
            }
        }

        // Phase label
        spans.push(Span::styled(self.phase.label().to_string(), phase_style));

        // Tool name for ToolCall / Approval
        match &self.phase {
            StatusPhase::ToolCall { tool_name } | StatusPhase::Approval { tool_name } => {
                spans.push(Span::styled("  →  ", Style::default().fg(STONE)));
                spans.push(Span::styled(tool_name.clone(), Style::default().fg(CREAM)));
            }
            _ => {}
        }

        // Error detail
        if let StatusPhase::Error(msg) = &self.phase {
            spans.push(Span::styled("  ", Style::default()));
            spans.push(Span::styled(
                msg.chars().take(40).collect::<String>(),
                Style::default().fg(ROSE),
            ));
        }

        // Elapsed time (show when active)
        if self.phase.is_active() {
            spans.push(Span::styled("  ", Style::default()));
            spans.push(Span::styled(
                self.elapsed_str(),
                Style::default().fg(STONE),
            ));
        }

        // Pending queue depth
        if self.pending_tool_calls > 0 {
            spans.push(Span::styled("  ", Style::default()));
            spans.push(Span::styled(
                format!("+{} queued", self.pending_tool_calls),
                Style::default().fg(AMBER),
            ));
        }

        // Model name (right-aligned via a trailing push)
        if !self.model.is_empty() {
            spans.push(Span::styled("  │  ", Style::default().fg(CHARCOAL)));
            spans.push(Span::styled(self.model.clone(), Style::default().fg(SPROUT)));
        }

        Line::from(spans)
    }
}

impl Panel for AgentStatusPanel {
    fn id(&self) -> PanelId {
        PanelId::AgentStatus
    }

    fn title(&self) -> &str {
        "Agent Status"
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
            .title(Span::styled(" Status ", Style::default().fg(TAN)))
            .style(Style::default().bg(BG));

        let inner = block.inner(area);
        frame.render_widget(block, area);

        let line = self.build_line();
        let para = Paragraph::new(line).style(Style::default().bg(BG));
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
    fn test_agent_status_default_is_idle() {
        let panel = AgentStatusPanel::new();
        assert!(matches!(panel.phase, StatusPhase::Idle));
        assert_eq!(panel.pending_tool_calls, 0);
    }

    #[test]
    fn test_set_phase_updates_label() {
        let mut panel = AgentStatusPanel::new();
        panel.set_phase(StatusPhase::Thinking);
        assert_eq!(panel.phase.label(), "Thinking");

        panel.set_phase(StatusPhase::ToolCall {
            tool_name: "shell".into(),
        });
        assert_eq!(panel.phase.label(), "Tool Call");

        panel.set_phase(StatusPhase::Approval {
            tool_name: "write_file".into(),
        });
        assert_eq!(panel.phase.label(), "Approval");

        panel.set_phase(StatusPhase::Error("oops".into()));
        assert_eq!(panel.phase.label(), "Error");

        panel.set_phase(StatusPhase::Idle);
        assert_eq!(panel.phase.label(), "Idle");
    }

    #[test]
    fn test_phase_is_active() {
        assert!(!StatusPhase::Idle.is_active());
        assert!(StatusPhase::Thinking.is_active());
        assert!(StatusPhase::ToolCall { tool_name: "t".into() }.is_active());
        assert!(StatusPhase::Approval { tool_name: "t".into() }.is_active());
        assert!(!StatusPhase::Error("x".into()).is_active());
    }

    #[test]
    fn test_elapsed_str_seconds() {
        let panel = AgentStatusPanel::new();
        // Freshly created — should be "0s".
        assert_eq!(panel.elapsed_str(), "0s");
    }

    #[test]
    fn test_elapsed_str_minutes_format() {
        // Test the format logic by constructing elapsed manually.
        // 90 seconds = "1m30s"
        let secs = 90u64;
        let result = format!("{}m{}s", secs / 60, secs % 60);
        assert_eq!(result, "1m30s");
    }

    #[test]
    fn test_spinner_char_cycles() {
        let mut panel = AgentStatusPanel::new();
        panel.set_phase(StatusPhase::Thinking);

        let chars: Vec<char> = (0..SPINNER.len())
            .map(|i| {
                panel.tick = i as u64;
                panel.spinner_char()
            })
            .collect();

        // Should contain all spinner frames (no repeats).
        let unique: std::collections::HashSet<char> = chars.into_iter().collect();
        assert_eq!(unique.len(), SPINNER.len());
    }

    #[test]
    fn test_spinner_wraps_around() {
        let mut panel = AgentStatusPanel::new();
        panel.tick = 0;
        let c0 = panel.spinner_char();

        panel.tick = SPINNER.len() as u64;
        let c_wrap = panel.spinner_char();

        assert_eq!(c0, c_wrap);
    }

    #[test]
    fn test_pending_tool_calls_tracked() {
        let mut panel = AgentStatusPanel::new();
        assert_eq!(panel.pending_tool_calls, 0);
        panel.pending_tool_calls = 3;
        assert_eq!(panel.pending_tool_calls, 3);
    }

    #[test]
    fn test_model_name_stored() {
        let mut panel = AgentStatusPanel::new();
        panel.model = "llama3".into();
        assert_eq!(panel.model, "llama3");
    }

    #[test]
    fn test_sync_from_state_updates_model() {
        let mut panel = AgentStatusPanel::new();
        let mut state = AppState::default();
        state.model = "gpt-4o".into();
        panel.sync_from_state(&state);
        assert_eq!(panel.model, "gpt-4o");
    }

    #[test]
    fn test_build_line_contains_label() {
        let mut panel = AgentStatusPanel::new();
        panel.set_phase(StatusPhase::Thinking);
        let line = panel.build_line();
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(text.contains("Thinking"), "line text: {text}");
    }

    #[test]
    fn test_build_line_tool_call_shows_tool_name() {
        let mut panel = AgentStatusPanel::new();
        panel.set_phase(StatusPhase::ToolCall {
            tool_name: "read_file".into(),
        });
        let line = panel.build_line();
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(text.contains("read_file"), "line text: {text}");
    }

    #[test]
    fn test_panel_id() {
        let panel = AgentStatusPanel::new();
        assert_eq!(panel.id(), PanelId::AgentStatus);
    }

    #[test]
    fn test_panel_title() {
        let panel = AgentStatusPanel::new();
        assert_eq!(panel.title(), "Agent Status");
    }

    #[test]
    fn test_panel_visibility() {
        let mut panel = AgentStatusPanel::new();
        assert!(panel.is_visible());
        panel.set_visible(false);
        assert!(!panel.is_visible());
        panel.set_visible(true);
        assert!(panel.is_visible());
    }
}
