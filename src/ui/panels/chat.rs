//! Chat panel — renders the full conversation history.
//!
//! Layout within the chat area:
//! - Scrollable list of message bubbles, newest at the bottom
//! - Thin dividers between conversation turns (user → assistant pairs)
//! - Tool cards inline within assistant messages
//! - Auto-scrolls to bottom on new messages unless the user has scrolled up

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    Frame,
    buffer::Buffer,
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Widget},
};
use chrono::Utc;

use crate::app::{
    state::{AppState, MessageRole, ToolCallStatus, UiPhase},
};
use crate::ui::theme::{Theme, AMBER, BG, CHARCOAL, CREAM, RUST_RED, SOIL, SPROUT, TAN};
use crate::ui::widgets::message_bubble::{bubble_height, MessageBubble};

use super::{Panel, PanelAction, PanelId};

// ── ChatPanel struct ──────────────────────────────────────────────────────────

/// The primary chat panel — conversation history.
#[derive(Debug, Default)]
pub struct ChatPanel {
    /// Vertical scroll offset (lines from the bottom).
    pub scroll: usize,
    /// Whether this panel is visible.
    visible: bool,
}

impl ChatPanel {
    pub fn new() -> Self {
        Self {
            scroll: 0,
            visible: true,
        }
    }
}

impl Panel for ChatPanel {
    fn id(&self) -> PanelId {
        PanelId::Chat
    }

    fn title(&self) -> &str {
        "Chat"
    }

    fn render(&self, frame: &mut Frame, area: Rect, focused: bool, state: &AppState) {
        let theme = Theme::default();
        let border_style = if focused {
            Style::default().fg(AMBER)
        } else {
            Style::default().fg(CHARCOAL)
        };

        // Draw border block.
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(border_style)
            .title(Span::styled(" Chat ", Style::default().fg(TAN)))
            .style(Style::default().bg(BG));

        let inner = block.inner(area);
        frame.render_widget(block, area);

        // Delegate message rendering to the shared draw function.
        let buf = frame.buffer_mut();
        draw_chat(buf, inner, state, &theme);
    }

    fn handle_key(&mut self, key: KeyEvent, state: &mut AppState) -> PanelAction {
        // Ctrl+C / Ctrl+Q are handled globally; don't intercept here.
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            return PanelAction::None;
        }
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                state.scroll_offset = state.scroll_offset.saturating_add(1);
                state.user_scrolled = true;
                PanelAction::None
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if state.scroll_offset > 0 {
                    state.scroll_offset -= 1;
                    if state.scroll_offset == 0 {
                        state.user_scrolled = false;
                    }
                }
                PanelAction::None
            }
            KeyCode::PageUp => {
                state.scroll_offset = state.scroll_offset.saturating_add(10);
                state.user_scrolled = true;
                PanelAction::None
            }
            KeyCode::PageDown => {
                if state.scroll_offset >= 10 {
                    state.scroll_offset -= 10;
                } else {
                    state.scroll_offset = 0;
                    state.user_scrolled = false;
                }
                PanelAction::None
            }
            _ => PanelAction::None,
        }
    }

    fn is_visible(&self) -> bool {
        self.visible
    }

    fn set_visible(&mut self, visible: bool) {
        self.visible = visible;
    }
}

// ── Public render entry point ─────────────────────────────────────────────────

/// Render the chat panel into `area` using the current [`AppState`].
///
/// Call this from the top-level view function.
pub fn render_chat(frame: &mut Frame, area: Rect, state: &AppState, theme: &Theme) {
    let buf = frame.buffer_mut();
    draw_chat(buf, area, state, theme);
}

// ── Core drawing function ─────────────────────────────────────────────────────

fn draw_chat(buf: &mut Buffer, area: Rect, state: &AppState, theme: &Theme) {
    // Fill background.
    for y in area.top()..area.bottom() {
        for x in area.left()..area.right() {
            buf[(x, y)].set_style(Style::default().bg(BG));
        }
    }

    match state.ui_phase {
        UiPhase::Welcome => draw_welcome(buf, area, theme),
        UiPhase::Active => draw_messages(buf, area, state, theme),
    }
}

// ── Welcome screen ────────────────────────────────────────────────────────────

fn draw_welcome(buf: &mut Buffer, area: Rect, _theme: &Theme) {
    if area.height < 5 || area.width < 20 {
        return;
    }
    let center_y = area.y + (area.height / 2).saturating_sub(2);

    let lines = vec![
        Line::from(vec![
            Span::raw("     "),
            Span::styled("🥔  Potato", Style::default().fg(AMBER)),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            "  Terminal-native AI agent — type a message to begin",
            Style::default().fg(SOIL),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled("  Ctrl+Q", Style::default().fg(TAN)),
            Span::styled(" to quit", Style::default().fg(SOIL)),
        ]),
    ];

    let render_height = lines.len() as u16;
    let render_area = Rect {
        x: area.x,
        y: center_y,
        width: area.width,
        height: render_height.min(area.bottom().saturating_sub(center_y)),
    };
    Paragraph::new(lines).render(render_area, buf);
}

// ── Message list ──────────────────────────────────────────────────────────────

/// A pre-flattened renderable row.
struct FlatRow {
    /// How many terminal lines this row occupies.
    height: u16,
    /// The pre-built lines.
    lines: Vec<Line<'static>>,
}

/// Build all rows from state (messages, dividers, tool cards).
fn build_rows(state: &AppState, theme: &Theme, width: u16) -> Vec<FlatRow> {
    let mut rows: Vec<FlatRow> = Vec::new();

    for (idx, msg) in state.messages.iter().enumerate() {
        // Divider before each user message (turn boundary).
        if msg.role == MessageRole::User && idx > 0 {
            let divider = Line::from(Span::styled(
                "─".repeat(width as usize),
                theme.divider(),
            ));
            rows.push(FlatRow {
                height: 1,
                lines: vec![divider],
            });
        }

        // Inline tool card (if any) before the message text.
        if let Some(ref tc) = msg.tool_call {
            let card_lines = build_tool_card_lines(tc, theme);
            let h = card_lines.len() as u16;
            rows.push(FlatRow {
                height: h.max(1),
                lines: card_lines,
            });
        }

        // Message bubble.
        let bubble = MessageBubble::new(msg, theme);
        let bh = bubble_height(msg, width);
        let text = bubble.to_text(width);
        rows.push(FlatRow {
            height: bh.max(text.lines.len() as u16).max(1),
            lines: text.lines,
        });
    }

    rows
}

fn draw_messages(buf: &mut Buffer, area: Rect, state: &AppState, theme: &Theme) {
    if area.height == 0 || area.width == 0 {
        return;
    }

    let rows = build_rows(state, theme, area.width);

    // Total virtual height.
    let total_height: u16 = rows.iter().map(|r| r.height).sum();

    // scroll_offset is lines-from-bottom. Convert to lines-from-top.
    let scroll_from_top: u16 = if total_height <= area.height {
        0
    } else {
        let max_scroll = total_height - area.height;
        max_scroll.saturating_sub(state.scroll_offset as u16)
    };

    // Render visible rows.
    let mut y_virtual: u16 = 0;
    for row in &rows {
        let row_end = y_virtual + row.height;

        // Before viewport
        if row_end <= scroll_from_top {
            y_virtual += row.height;
            continue;
        }

        // After viewport
        if y_virtual >= scroll_from_top + area.height {
            break;
        }

        // Lines of this row that are clipped from the top.
        let clip_top = scroll_from_top.saturating_sub(y_virtual) as usize;
        // Y position on screen.
        let screen_y = area.y + y_virtual.saturating_sub(scroll_from_top);
        let available = area.bottom().saturating_sub(screen_y);
        if available == 0 {
            y_virtual += row.height;
            continue;
        }

        // Render each visible line of this row.
        for (li, line) in row.lines.iter().enumerate().skip(clip_top) {
            let ly = screen_y + (li - clip_top) as u16;
            if ly >= area.bottom() {
                break;
            }
            // Render this Line into the buffer at (area.x, ly).
            render_line_at(buf, line, area.x, ly, area.width);
        }

        y_virtual += row.height;
    }

    // Scroll indicator
    if state.user_scrolled && state.scroll_offset > 0 {
        let label = format!(" ↓ {} below ", state.scroll_offset);
        let lx = area.right().saturating_sub(label.len() as u16 + 1);
        let ly = area.bottom().saturating_sub(1);
        let style = Style::default().fg(AMBER).bg(CHARCOAL);
        for (i, ch) in label.chars().enumerate() {
            let cx = lx + i as u16;
            if cx >= area.right() {
                break;
            }
            buf[(cx, ly)].set_char(ch).set_style(style);
        }
    }
}

// ── Tool card line builder ────────────────────────────────────────────────────

fn build_tool_card_lines(
    tc: &crate::app::state::ToolCallInfo,
    theme: &Theme,
) -> Vec<Line<'static>> {
    let status_style = match tc.status {
        ToolCallStatus::Running => Style::default().fg(AMBER),
        ToolCallStatus::Done => Style::default().fg(SPROUT),
        ToolCallStatus::Failed => Style::default().fg(RUST_RED),
    };

    let icon = match tc.status {
        ToolCallStatus::Running => "●",
        ToolCallStatus::Done => "✓",
        ToolCallStatus::Failed => "✗",
    };

    let elapsed = Utc::now()
        .signed_duration_since(tc.started_at)
        .num_milliseconds();
    let duration = if elapsed < 1000 {
        format!("{}ms", elapsed)
    } else {
        format!("{:.1}s", elapsed as f64 / 1000.0)
    };

    let hint = if tc.expanded { "▾ " } else { "▸ " };

    let header = Line::from(vec![
        Span::styled(hint.to_string(), theme.muted()),
        Span::styled(format!("{} ", icon), status_style),
        Span::styled(tc.tool_name.clone(), status_style),
        Span::raw("  "),
        Span::styled(duration, theme.muted()),
    ]);

    if !tc.expanded {
        return vec![header];
    }

    let mut lines = vec![header];
    lines.push(Line::from(Span::styled("  args:", theme.muted())));
    for arg_line in tc.args.lines().take(10) {
        lines.push(Line::from(Span::styled(
            format!("    {}", arg_line),
            Style::default().fg(CREAM),
        )));
    }
    if let Some(ref output) = tc.output {
        lines.push(Line::from(Span::styled("  output:", theme.muted())));
        for out_line in output.lines().take(6) {
            lines.push(Line::from(Span::styled(
                format!("    {}", out_line),
                Style::default().fg(CREAM),
            )));
        }
    }
    lines
}

// ── Line renderer helper ──────────────────────────────────────────────────────

/// Write a [`Line`] into the buffer at a specific (x, y) position.
fn render_line_at(buf: &mut Buffer, line: &Line<'_>, x: u16, y: u16, max_width: u16) {
    let mut cx = x;
    for span in &line.spans {
        for ch in span.content.chars() {
            if cx >= x + max_width {
                return;
            }
            buf[(cx, y)].set_char(ch).set_style(span.style);
            cx += 1;
        }
    }
}
