//! Message bubble widget — renders a single chat message with role styling.
//!
//! Each bubble displays:
//! - A role icon / prefix (❯ for user, 🥔 for assistant, ! for system/error)
//! - Word-wrapped content with markdown-lite formatting
//! - A muted timestamp on the right margin

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Paragraph, Widget, Wrap},
};
use unicode_width::UnicodeWidthStr;

use crate::app::state::{ChatMessage, MessageRole};
use crate::ui::theme::{Theme, AMBER, BG, CHARCOAL, CREAM, RUST_RED, SOIL, TAN};

/// Rendered height estimate: used by the chat panel for scroll calculations.
///
/// Returns the number of terminal lines this bubble will occupy for the given
/// `width`.
pub fn bubble_height(msg: &ChatMessage, width: u16) -> u16 {
    if width == 0 {
        return 1;
    }
    // Prefix width: "❯ " or "🥔 " — treat as 2 columns.
    let content_width = (width as usize).saturating_sub(2).max(1);
    let content_lines = word_wrap_count(&msg.content, content_width);
    // 1 line for timestamp + content, at minimum 1.
    (content_lines.max(1) as u16) + 1 // +1 for timestamp row
}

/// Count the number of wrapped lines for a string in `width` columns.
fn word_wrap_count(text: &str, width: usize) -> usize {
    if text.is_empty() {
        return 1;
    }
    let mut count = 0usize;
    for line in text.lines() {
        if line.is_empty() {
            count += 1;
            continue;
        }
        let line_width = UnicodeWidthStr::width(line);
        count += (line_width + width - 1) / width;
    }
    count.max(1)
}

// ── Markdown-lite parser ──────────────────────────────────────────────────────

/// Parse a line of text into styled [`Span`]s supporting:
/// - `**bold**`
/// - `` `code` ``
fn parse_inline(text: &str, base_style: Style, theme: &Theme) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let mut remaining = text.to_string();

    while !remaining.is_empty() {
        // Bold: **...**
        if let Some(start) = remaining.find("**") {
            let after_open = start + 2;
            if let Some(end_rel) = remaining[after_open..].find("**") {
                let end = after_open + end_rel;
                // Text before the bold
                if start > 0 {
                    spans.push(Span::styled(
                        remaining[..start].to_string(),
                        base_style,
                    ));
                }
                // Bold content
                spans.push(Span::styled(
                    remaining[after_open..end].to_string(),
                    base_style.add_modifier(Modifier::BOLD),
                ));
                remaining = remaining[end + 2..].to_string();
                continue;
            }
        }

        // Inline code: `...`
        if let Some(start) = remaining.find('`') {
            let after_open = start + 1;
            if let Some(end_rel) = remaining[after_open..].find('`') {
                let end = after_open + end_rel;
                if start > 0 {
                    spans.push(Span::styled(remaining[..start].to_string(), base_style));
                }
                spans.push(Span::styled(
                    remaining[after_open..end].to_string(),
                    theme.inline_code(),
                ));
                remaining = remaining[end + 1..].to_string();
                continue;
            }
        }

        // No more markdown — emit the rest as plain.
        spans.push(Span::styled(remaining.clone(), base_style));
        break;
    }

    spans
}

// ── MessageBubble widget ──────────────────────────────────────────────────────

/// Renders a single user or assistant message as a styled bubble.
///
/// Supports word-wrapping and markdown-lite inline formatting.
pub struct MessageBubble<'a> {
    /// The message to render.
    pub message: &'a ChatMessage,
    /// Application theme.
    pub theme: &'a Theme,
    /// Whether this bubble is the last in the list (for divider placement).
    pub is_last: bool,
}

impl<'a> MessageBubble<'a> {
    /// Create a new [`MessageBubble`].
    pub fn new(message: &'a ChatMessage, theme: &'a Theme) -> Self {
        Self {
            message,
            theme,
            is_last: false,
        }
    }

    /// Mark this bubble as the last in the list.
    pub fn last(mut self) -> Self {
        self.is_last = true;
        self
    }

    /// Build the ratatui [`Text`] for this bubble.
    pub fn to_text(&self, width: u16) -> Text<'static> {
        let msg = self.message;
        let theme = self.theme;

        let (prefix, base_style): (&str, Style) = match msg.role {
            MessageRole::User => ("❯ ", theme.user_message()),
            MessageRole::Assistant => ("🥔 ", theme.assistant_message()),
            MessageRole::System => ("ℹ ", theme.system_message()),
            MessageRole::Error => ("✖ ", theme.error_message()),
        };

        let timestamp = msg.timestamp.format("%H:%M").to_string();
        let ts_width = timestamp.len() + 1;
        let content_width = (width as usize)
            .saturating_sub(prefix.len())
            .saturating_sub(ts_width)
            .max(1);

        let mut lines: Vec<Line<'static>> = Vec::new();

        // Build content lines
        let text_lines: Vec<&str> = msg.content.lines().collect();
        let text_lines = if text_lines.is_empty() {
            vec![""]
        } else {
            text_lines
        };

        for (i, text_line) in text_lines.iter().enumerate() {
            let is_first = i == 0;

            // Wrap this line
            let wrapped = soft_wrap(text_line, content_width);
            for (j, wrapped_line) in wrapped.iter().enumerate() {
                let line_is_first = is_first && j == 0;
                let mut spans: Vec<Span<'static>> = Vec::new();

                // Prefix on the very first line only
                if line_is_first {
                    spans.push(Span::styled(prefix.to_string(), base_style));
                } else {
                    // Indent subsequent lines to align with content
                    spans.push(Span::raw("  ".to_string()));
                }

                // Parse inline markdown for the content
                let content_spans = parse_inline(wrapped_line, base_style, theme);
                spans.extend(content_spans);

                lines.push(Line::from(spans));
            }
        }

        // Timestamp line (right-aligned, muted)
        let pad = " ".repeat(
            (width as usize)
                .saturating_sub(prefix.len())
                .saturating_sub(timestamp.len()),
        );
        lines.push(Line::from(vec![
            Span::raw(pad),
            Span::styled(timestamp, theme.muted()),
        ]));

        Text::from(lines)
    }
}

impl<'a> Widget for MessageBubble<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let text = self.to_text(area.width);
        Paragraph::new(text)
            .wrap(Wrap { trim: false })
            .render(area, buf);
    }
}

// ── Soft-wrap helper ──────────────────────────────────────────────────────────

/// Break `text` into lines of at most `width` display columns.
fn soft_wrap(text: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![text.to_string()];
    }
    if text.is_empty() {
        return vec![String::new()];
    }

    let mut lines = Vec::new();
    let mut current = String::new();
    let mut current_width = 0usize;

    for word in text.split_whitespace() {
        let word_w = UnicodeWidthStr::width(word);
        if current.is_empty() {
            current.push_str(word);
            current_width = word_w;
        } else if current_width + 1 + word_w <= width {
            current.push(' ');
            current.push_str(word);
            current_width += 1 + word_w;
        } else {
            lines.push(current.clone());
            current = word.to_string();
            current_width = word_w;
        }
    }
    if !current.is_empty() || lines.is_empty() {
        lines.push(current);
    }
    lines
}
