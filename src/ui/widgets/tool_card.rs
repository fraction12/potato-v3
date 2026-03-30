//! Tool card widget — compact, collapsible display of a tool call.
//!
//! **Collapsed** (default): one line showing status icon, tool name, duration.
//!
//! ```text
//!  ● read_file  1.2s
//! ```
//!
//! **Expanded**: bordered block with arguments and output below the header.

use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Layout, Rect},
    style::Style,
    text::{Line, Span, Text},
    widgets::{Block, Borders, Paragraph, Widget, Wrap},
};

use crate::app::state::{ToolCallInfo, ToolCallStatus};
use crate::ui::theme::{AMBER, CHARCOAL, CREAM, RUST_RED, SPROUT, Theme};

/// Status icon strings for each tool call state.
const ICON_RUNNING: &str = "●";
const ICON_DONE: &str = "✓";
const ICON_FAILED: &str = "✗";

/// Returns the appropriate status icon for a tool call.
fn status_icon(status: &ToolCallStatus) -> &'static str {
    match status {
        ToolCallStatus::Running => ICON_RUNNING,
        ToolCallStatus::Done => ICON_DONE,
        ToolCallStatus::Failed => ICON_FAILED,
    }
}

/// Returns the approximate height this card will occupy.
///
/// - Collapsed: 1 line
/// - Expanded: 2 lines header + border + args lines + output lines
pub fn card_height(tc: &ToolCallInfo, width: u16) -> u16 {
    if !tc.expanded {
        return 1;
    }
    let inner_w = (width as usize).saturating_sub(2).max(1);
    // Header line + args block header
    let mut h: u16 = 3; // top border + header + separator
    h += args_line_count(&tc.args, inner_w) as u16;
    if let Some(ref out) = tc.output {
        h += 1; // "Output:" label
        h += output_line_count(out, inner_w) as u16;
    }
    h += 1; // bottom border
    h
}

fn args_line_count(args: &str, _width: usize) -> usize {
    if args.is_empty() {
        return 1;
    }
    let lines: usize = args.lines().count();
    lines.max(1)
}

fn output_line_count(output: &str, _width: usize) -> usize {
    output.lines().count().min(8).max(1)
}

// ── ToolCard widget ───────────────────────────────────────────────────────────

/// Renders a tool call as a collapsible card.
pub struct ToolCard<'a> {
    /// The tool call data to display.
    pub tool_call: &'a ToolCallInfo,
    /// Application theme.
    pub theme: &'a Theme,
}

impl<'a> ToolCard<'a> {
    /// Create a new [`ToolCard`].
    pub fn new(tool_call: &'a ToolCallInfo, theme: &'a Theme) -> Self {
        Self { tool_call, theme }
    }

    /// Border / icon style based on tool call status.
    fn status_style(&self) -> Style {
        match self.tool_call.status {
            ToolCallStatus::Running => self.theme.tool_running(),
            ToolCallStatus::Done => self.theme.tool_done(),
            ToolCallStatus::Failed => self.theme.tool_failed(),
        }
    }

    /// Format the elapsed duration as a short string.
    fn duration_str(&self) -> String {
        let elapsed = chrono::Utc::now()
            .signed_duration_since(self.tool_call.started_at)
            .num_milliseconds();
        if elapsed < 1000 {
            format!("{}ms", elapsed)
        } else {
            format!("{:.1}s", elapsed as f64 / 1000.0)
        }
    }

    /// Build the single collapsed line.
    fn collapsed_line(&self) -> Line<'static> {
        let tc = self.tool_call;
        let icon = status_icon(&tc.status);
        let style = self.status_style();
        let duration = self.duration_str();

        // Expansion hint
        let hint = if tc.expanded { "▾ " } else { "▸ " };

        Line::from(vec![
            Span::styled(hint.to_string(), self.theme.muted()),
            Span::styled(format!("{} ", icon), style),
            Span::styled(tc.tool_name.clone(), style),
            Span::raw("  "),
            Span::styled(duration, self.theme.muted()),
        ])
    }
}

impl<'a> Widget for ToolCard<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if !self.tool_call.expanded || area.height <= 2 {
            // Collapsed: single line
            let line = self.collapsed_line();
            Paragraph::new(line).render(area, buf);
            return;
        }

        // Expanded: bordered block
        let tc = self.tool_call;
        let border_style = self.status_style();

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(border_style)
            .title(Line::from(vec![
                Span::styled(
                    format!(" {} {} ", status_icon(&tc.status), tc.tool_name),
                    border_style,
                ),
                Span::styled(self.duration_str(), self.theme.muted()),
                Span::raw(" "),
            ]));

        let inner = block.inner(area);
        block.render(area, buf);

        // Build inner content
        let mut lines: Vec<Line<'static>> = Vec::new();

        // Arguments section
        lines.push(Line::from(Span::styled("args:", self.theme.muted())));
        for arg_line in tc.args.lines().take(20) {
            lines.push(Line::from(Span::styled(
                format!("  {}", arg_line),
                Style::default().fg(CREAM),
            )));
        }

        // Output section
        if let Some(ref output) = tc.output {
            lines.push(Line::from(Span::styled("output:", self.theme.muted())));
            for out_line in output.lines().take(8) {
                lines.push(Line::from(Span::styled(
                    format!("  {}", out_line),
                    Style::default().fg(CREAM),
                )));
            }
        }

        Paragraph::new(Text::from(lines))
            .wrap(Wrap { trim: false })
            .render(inner, buf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::state::{ToolCallInfo, ToolCallStatus};

    fn make_tc(
        status: ToolCallStatus,
        args: &str,
        output: Option<&str>,
        expanded: bool,
    ) -> ToolCallInfo {
        ToolCallInfo {
            tool_name: "read_file".into(),
            args: args.into(),
            output: output.map(|s| s.into()),
            status,
            started_at: chrono::Utc::now(),
            expanded,
        }
    }

    // ── status_icon ──

    #[test]
    fn status_icon_running() {
        assert_eq!(status_icon(&ToolCallStatus::Running), "●");
    }

    #[test]
    fn status_icon_done() {
        assert_eq!(status_icon(&ToolCallStatus::Done), "✓");
    }

    #[test]
    fn status_icon_failed() {
        assert_eq!(status_icon(&ToolCallStatus::Failed), "✗");
    }

    // ── args_line_count ──

    #[test]
    fn args_line_count_empty() {
        assert_eq!(args_line_count("", 80), 1);
    }

    #[test]
    fn args_line_count_single_line() {
        assert_eq!(args_line_count("path: /foo/bar", 80), 1);
    }

    #[test]
    fn args_line_count_multiline() {
        assert_eq!(args_line_count("a\nb\nc", 80), 3);
    }

    // ── output_line_count ──

    #[test]
    fn output_line_count_empty() {
        // empty string has 0 lines().count(), clamped to max(1)
        assert_eq!(output_line_count("", 80), 1);
    }

    #[test]
    fn output_line_count_short() {
        assert_eq!(output_line_count("line1\nline2\nline3", 80), 3);
    }

    #[test]
    fn output_line_count_capped_at_8() {
        let long = (1..=20)
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(output_line_count(&long, 80), 8);
    }

    // ── card_height ──

    #[test]
    fn card_height_collapsed() {
        let tc = make_tc(ToolCallStatus::Running, "", None, false);
        assert_eq!(card_height(&tc, 80), 1);
    }

    #[test]
    fn card_height_expanded_no_output() {
        // 3 (top border + header + separator) + 1 (empty args = 1 line) + 1 (bottom border) = 5
        let tc = make_tc(ToolCallStatus::Done, "", None, true);
        assert_eq!(card_height(&tc, 80), 5);
    }

    #[test]
    fn card_height_expanded_with_args_and_output() {
        let tc = make_tc(ToolCallStatus::Done, "a\nb\nc", Some("out1\nout2"), true);
        // 3 + 3 (args) + 1 (output label) + 2 (output lines) + 1 (bottom border) = 10
        assert_eq!(card_height(&tc, 80), 10);
    }

    #[test]
    fn card_height_expanded_output_capped() {
        let long_out = (1..=20)
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let tc = make_tc(ToolCallStatus::Done, "arg", Some(&long_out), true);
        // 3 + 1 (args) + 1 (output label) + 8 (capped) + 1 (bottom) = 14
        assert_eq!(card_height(&tc, 80), 14);
    }

    // ── ToolCard ──

    #[test]
    fn tool_card_status_style_maps_correctly() {
        let theme = Theme::default();
        for (status, expected_style) in [
            (ToolCallStatus::Running, theme.tool_running()),
            (ToolCallStatus::Done, theme.tool_done()),
            (ToolCallStatus::Failed, theme.tool_failed()),
        ] {
            let tc = make_tc(status, "", None, false);
            let card = ToolCard::new(&tc, &theme);
            assert_eq!(card.status_style(), expected_style);
        }
    }

    #[test]
    fn collapsed_line_contains_tool_name() {
        let theme = Theme::default();
        let tc = make_tc(ToolCallStatus::Done, "", None, false);
        let card = ToolCard::new(&tc, &theme);
        let line = card.collapsed_line();
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(text.contains("read_file"));
        assert!(text.contains("✓"));
    }

    #[test]
    fn collapsed_line_shows_expand_hint() {
        let theme = Theme::default();
        let mut tc = make_tc(ToolCallStatus::Running, "", None, false);
        let card = ToolCard::new(&tc, &theme);
        let line = card.collapsed_line();
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(text.contains("▸"), "collapsed should show ▸");

        tc.expanded = true;
        let card = ToolCard::new(&tc, &theme);
        let line = card.collapsed_line();
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(text.contains("▾"), "expanded should show ▾");
    }

    #[test]
    fn duration_str_format() {
        let theme = Theme::default();
        let tc = make_tc(ToolCallStatus::Running, "", None, false);
        let card = ToolCard::new(&tc, &theme);
        let dur = card.duration_str();
        // Just created, should be <1000ms
        assert!(dur.ends_with("ms"), "expected ms suffix, got: {dur}");
    }
}
