//! Approval bar widget — inline prompt for approving or denying a tool call.
//!
//! Renders an amber header line with the tool name and keybind hints,
//! followed by a preview of the arguments (and optional diff/preview text).
//!
//! ```text
//! ╔══ ⚠ Approval required: write_file ════════════════════════╗
//! ║  path: /tmp/foo.txt                                       ║
//! ║  [y] approve  [n] deny  [a] approve all                   ║
//! ╚═══════════════════════════════════════════════════════════╝
//! ```

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::Style,
    text::{Line, Span, Text},
    widgets::{Block, Borders, Clear, Paragraph, Widget, Wrap},
};

use crate::app::state::PendingApproval;
use crate::ui::theme::{AMBER, BG, CHARCOAL, CREAM, ROSE, RUST_RED, Theme};

/// Renders the pending-approval prompt over the input area.
pub struct ApprovalBar<'a> {
    /// The pending approval data.
    pub approval: &'a PendingApproval,
    /// Application theme.
    pub theme: &'a Theme,
}

impl<'a> ApprovalBar<'a> {
    /// Create a new [`ApprovalBar`].
    pub fn new(approval: &'a PendingApproval, theme: &'a Theme) -> Self {
        Self { approval, theme }
    }
}

impl<'a> Widget for ApprovalBar<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        // Clear the background so overlapping layers don't bleed through.
        Clear.render(area, buf);

        let approval = self.approval;
        let theme = self.theme;

        // Amber-bordered block
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(theme.approval_header())
            .title(Line::from(vec![
                Span::styled(" ⚠ Approval required: ", theme.approval_header()),
                Span::styled(approval.tool_name.clone(), theme.approval_header()),
                Span::styled(" ", theme.approval_header()),
            ]))
            .style(Style::default().bg(CHARCOAL));

        let inner = block.inner(area);
        block.render(area, buf);

        let mut lines: Vec<Line<'static>> = Vec::new();

        // Show a few lines of args
        for arg_line in approval.args.lines().take(4) {
            lines.push(Line::from(Span::styled(
                format!("  {}", arg_line),
                Style::default().fg(CREAM),
            )));
        }

        // Show diff/preview if available
        if let Some(ref preview) = approval.preview {
            lines.push(Line::from(Span::styled(
                "  ─── preview ───",
                theme.muted(),
            )));
            for preview_line in preview.lines().take(4) {
                let style = if preview_line.starts_with('+') {
                    Style::default().fg(crate::ui::theme::SPROUT)
                } else if preview_line.starts_with('-') {
                    Style::default().fg(ROSE)
                } else {
                    Style::default().fg(CREAM)
                };
                lines.push(Line::from(Span::styled(
                    format!("  {}", preview_line),
                    style,
                )));
            }
        }

        // Keybind hints
        lines.push(Line::from(vec![
            Span::raw("  "),
            Span::styled("[y]", theme.approval_header()),
            Span::raw(" approve  "),
            Span::styled("[n]", Style::default().fg(ROSE)),
            Span::raw(" deny  "),
            Span::styled("[a]", theme.approval_header()),
            Span::raw(" approve all"),
        ]));

        Paragraph::new(Text::from(lines))
            .wrap(Wrap { trim: true })
            .render(inner, buf);
    }
}
