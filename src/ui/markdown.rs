//! Markdown-aware line rendering utilities for the chat panel.
//!
//! This module provides lightweight, single-line markdown parsing that converts
//! common markdown constructs into styled ratatui [`Span`]s. It does *not* parse
//! multi-line constructs — those are handled at the caller level using
//! [`is_code_fence`].

use ratatui::{
    style::{Modifier, Style},
    text::Span,
};

use crate::ui::theme::{BRASS, CHARCOAL, CREAM, SPROUT};

// ── Public API ────────────────────────────────────────────────────────────────

/// Parse a single line of (simplified) Markdown and return a list of styled
/// [`Span`]s ready to be placed into a [`ratatui::text::Line`].
///
/// Supported constructs:
/// - `# Heading` → Sprout + Bold
/// - `## Heading` / `### Heading` → Brown + Bold
/// - `**bold**` → Bold
/// - `*italic*` → Italic
/// - `` `code` `` → Charcoal bg + Cream fg (inline code)
/// - Plain text → Cream
#[must_use]
pub fn render_markdown_line(line: &str) -> Vec<Span<'static>> {
    // ── Heading shortcuts (must be the first thing on the line) ──────────────
    if line.starts_with("### ") {
        let text = line[4..].to_string();
        return vec![Span::styled(
            text,
            Style::default().fg(BRASS).add_modifier(Modifier::BOLD),
        )];
    }
    if line.starts_with("## ") {
        let text = line[3..].to_string();
        return vec![Span::styled(
            text,
            Style::default().fg(BRASS).add_modifier(Modifier::BOLD),
        )];
    }
    if line.starts_with("# ") {
        let text = line[2..].to_string();
        return vec![Span::styled(
            text,
            Style::default().fg(SPROUT).add_modifier(Modifier::BOLD),
        )];
    }

    // ── Inline parse: bold / italic / inline-code ─────────────────────────
    parse_inline(line)
}

/// Returns `true` if `line` starts with three back-ticks (a code fence).
#[must_use]
pub fn is_code_fence(line: &str) -> bool {
    line.starts_with("```")
}

/// Extracts the language identifier from an opening code fence.
///
/// E.g. `"```rust"` → `"rust"`, `"```"` → `""`.
#[must_use]
pub fn extract_lang(line: &str) -> &str {
    if line.starts_with("```") {
        line[3..].trim()
    } else {
        ""
    }
}

// ── Inline parser ─────────────────────────────────────────────────────────────

/// State machine that parses inline markdown from a single text line.
fn parse_inline(line: &str) -> Vec<Span<'static>> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    let bytes = line.as_bytes();
    let len = line.len();
    let mut buf = String::new(); // current plain-text accumulation

    macro_rules! flush_plain {
        () => {
            if !buf.is_empty() {
                spans.push(Span::styled(buf.clone(), Style::default().fg(CREAM)));
                buf.clear();
            }
        };
    }

    let mut i = 0usize;
    while i < len {
        // ── `inline code` ─────────────────────────────────────────────────
        if bytes[i] == b'`' {
            // Find the closing backtick.
            if let Some(close) = line[i + 1..].find('`') {
                let code = line[i + 1..i + 1 + close].to_string();
                flush_plain!();
                spans.push(Span::styled(code, Style::default().fg(CREAM).bg(CHARCOAL)));
                i = i + 1 + close + 1;
                continue;
            }
        }

        // ── **bold** ──────────────────────────────────────────────────────
        if i + 1 < len && bytes[i] == b'*' && bytes[i + 1] == b'*' {
            if let Some(close) = line[i + 2..].find("**") {
                let text = line[i + 2..i + 2 + close].to_string();
                flush_plain!();
                spans.push(Span::styled(
                    text,
                    Style::default().add_modifier(Modifier::BOLD).fg(CREAM),
                ));
                i = i + 2 + close + 2;
                continue;
            }
        }

        // ── *italic* ──────────────────────────────────────────────────────
        if bytes[i] == b'*' {
            if let Some(close) = line[i + 1..].find('*') {
                let text = line[i + 1..i + 1 + close].to_string();
                flush_plain!();
                spans.push(Span::styled(
                    text,
                    Style::default().add_modifier(Modifier::ITALIC).fg(CREAM),
                ));
                i = i + 1 + close + 1;
                continue;
            }
        }

        // ── Plain character ───────────────────────────────────────────────
        // Find the length of the current character (UTF-8 safe).
        let ch_len = line[i..].chars().next().map(|c| c.len_utf8()).unwrap_or(1);
        buf.push_str(&line[i..i + ch_len]);
        i += ch_len;
    }
    flush_plain!();

    if spans.is_empty() {
        spans.push(Span::styled(String::new(), Style::default().fg(CREAM)));
    }

    spans
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::{Color, Modifier, Style};

    fn span_style(s: &Span) -> Style {
        s.style
    }

    // ── render_markdown_line tests ────────────────────────────────────────────

    #[test]
    fn plain_text_is_cream() {
        let spans = render_markdown_line("hello world");
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].content, "hello world");
        assert_eq!(spans[0].style.fg, Some(CREAM));
    }

    #[test]
    fn bold_text_is_bold() {
        let spans = render_markdown_line("**bold**");
        assert_eq!(spans.len(), 1);
        assert!(
            spans[0].style.add_modifier.contains(Modifier::BOLD),
            "expected BOLD modifier"
        );
        assert_eq!(spans[0].content, "bold");
    }

    #[test]
    fn italic_text_is_italic() {
        let spans = render_markdown_line("*italic*");
        assert_eq!(spans.len(), 1);
        assert!(
            spans[0].style.add_modifier.contains(Modifier::ITALIC),
            "expected ITALIC modifier"
        );
        assert_eq!(spans[0].content, "italic");
    }

    #[test]
    fn inline_code_has_charcoal_bg() {
        let spans = render_markdown_line("`code`");
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].content, "code");
        assert_eq!(spans[0].style.bg, Some(CHARCOAL));
        assert_eq!(spans[0].style.fg, Some(CREAM));
    }

    #[test]
    fn h1_is_sprout_and_bold() {
        let spans = render_markdown_line("# Title");
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].content, "Title");
        assert_eq!(spans[0].style.fg, Some(SPROUT));
        assert!(spans[0].style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn h2_is_brass_and_bold() {
        let spans = render_markdown_line("## Sub");
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].content, "Sub");
        assert_eq!(spans[0].style.fg, Some(BRASS));
        assert!(spans[0].style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn h3_is_brass_and_bold() {
        let spans = render_markdown_line("### Sub sub");
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].content, "Sub sub");
        assert_eq!(spans[0].style.fg, Some(BRASS));
        assert!(spans[0].style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn mixed_inline_produces_multiple_spans() {
        let spans = render_markdown_line("hello **world** foo");
        // Should produce: "hello " (plain), "world" (bold), " foo" (plain)
        assert!(
            spans.len() >= 3,
            "expected at least 3 spans, got {}",
            spans.len()
        );
        let bold = spans.iter().find(|s| s.content == "world").unwrap();
        assert!(bold.style.add_modifier.contains(Modifier::BOLD));
    }

    // ── is_code_fence tests ───────────────────────────────────────────────────

    #[test]
    fn is_code_fence_detects_triple_backtick() {
        assert!(is_code_fence("```"));
        assert!(is_code_fence("```rust"));
        assert!(is_code_fence("```python some stuff"));
        assert!(!is_code_fence("``not a fence"));
        assert!(!is_code_fence("  ```indented"));
        assert!(!is_code_fence("plain text"));
    }

    // ── extract_lang tests ────────────────────────────────────────────────────

    #[test]
    fn extract_lang_returns_language() {
        assert_eq!(extract_lang("```rust"), "rust");
        assert_eq!(extract_lang("```python"), "python");
        assert_eq!(extract_lang("```"), "");
        assert_eq!(extract_lang("```  "), "");
    }

    #[test]
    fn extract_lang_with_spaces() {
        // trim() means leading/trailing spaces around the lang are stripped
        assert_eq!(extract_lang("``` rust "), "rust");
    }
}
