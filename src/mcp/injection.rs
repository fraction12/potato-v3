//! PTY injection engine for inter-session notifications.
//!
//! Writes formatted text into a target pane's PTY stdin so Claude "sees" the
//! message as if a human typed or pasted it.
//!
//! # Safety
//!
//! Never inject during `approval_pending` state — a stray paste could
//! accidentally confirm or deny a tool approval.

use crate::app::pane::PaneManager;
use crate::app::state::AgentStatus;

/// A request to inject a message into a target pane's PTY.
///
/// Sent from the MCP bridge (async/tokio) to the main event loop which
/// owns the `PaneManager` and PTY handles.
#[derive(Debug, Clone)]
pub struct InjectRequest {
    /// Source pane id.
    pub from_pane: u64,
    /// Source pane role (if any).
    pub from_role: Option<String>,
    /// Target pane id.
    pub to_pane: u64,
    /// Message content.
    pub content: String,
}

/// Format a message notification for PTY injection.
///
/// The format uses a clearly delimited block so Claude can distinguish it
/// from user-typed content:
///
/// ```text
/// [Potato: message from Pane 0 (architect)]
/// Hey, I finished the API design. Check shared context for the schema.
/// [/Potato]
/// ```
/// Format a message notification for PTY injection.
///
/// Sends raw text followed by `\r` (Enter) to submit. Bracketed paste
/// is intentionally NOT used because Claude Code's Ink raw mode does not
/// treat bracketed paste as a submit trigger (see broadcast fix 112096f).
///
/// ```text
/// [Potato: message from Pane 0 (architect)]
/// Hey, I finished the API design. Check shared context for the schema.
/// [/Potato]
/// ```
pub fn format_notification(from_pane: u64, from_role: Option<&str>, content: &str) -> String {
    let role_suffix = from_role
        .filter(|r| !r.is_empty())
        .map(|r| format!(" ({r})"))
        .unwrap_or_default();

    // Sanitize control characters from content to prevent injection attacks.
    // A \r in the content would submit arbitrary input to Claude's PTY.
    // A \n would be mishandled by Claude Code's Ink raw mode.
    // Strip all C0 control chars, then re-add only the trailing \r to submit.
    let sanitized: String = content
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let flat_content = sanitized.replace("  ", " ");
    format!("[Potato: Pane {from_pane}{role_suffix}] {flat_content}\r")
}

/// Attempt to inject a formatted notification into a target pane's PTY.
///
/// Returns `Ok(true)` if injected, `Ok(false)` if the target pane is in a
/// state that blocks injection (approval pending, not found, no PTY), and
/// `Err` on I/O failure.
pub fn inject_into_pane(
    panes: &mut PaneManager,
    target_pane_index: usize,
    text: &str,
) -> Result<bool, String> {
    let pane = panes
        .get_mut(target_pane_index)
        .ok_or_else(|| format!("target pane index {target_pane_index} not found"))?;

    // Guard: never inject during approval_pending.
    if pane.session.approval_pending.is_some() {
        return Ok(false);
    }

    // Guard: skip if the PTY isn't alive.
    let pty = pane
        .pty
        .as_mut()
        .ok_or_else(|| "target pane has no PTY".to_string())?;

    if pty.child_exited() {
        return Ok(false);
    }

    pty.write_input(text.as_bytes())
        .map_err(|e| format!("PTY write failed: {e}"))?;

    Ok(true)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_notification_basic() {
        let msg = format_notification(0, None, "hello from pane 0");
        assert!(msg.contains("[Potato: Pane 0]"));
        assert!(msg.contains("hello from pane 0"));
        assert!(msg.ends_with('\r'));
    }

    #[test]
    fn format_notification_with_role() {
        let msg = format_notification(1, Some("architect"), "the schema is ready");
        assert!(msg.contains("[Potato: Pane 1 (architect)]"));
        assert!(msg.contains("the schema is ready"));
    }

    #[test]
    fn format_notification_empty_role_is_omitted() {
        let msg = format_notification(0, Some(""), "test");
        assert!(msg.contains("[Potato: Pane 0]"));
        assert!(!msg.contains("()"));
    }

    #[test]
    fn format_notification_single_line_no_newlines() {
        let msg = format_notification(0, None, "test content");
        // Must not contain any \n — only \r at the end to submit.
        assert!(!msg.contains('\n'), "must be single-line for Claude Code raw mode");
        assert!(msg.ends_with('\r'), "missing trailing carriage return");
        assert!(msg.contains("[Potato:"));
        assert!(msg.contains("test content"));
    }

    #[test]
    fn format_notification_multiline_flattened() {
        let msg = format_notification(0, None, "line1\nline2\nline3");
        assert!(!msg.contains('\n'), "newlines must be flattened");
        // Control chars become spaces, double spaces collapsed
        assert!(msg.contains("line1 line2 line3"));
        assert!(msg.ends_with('\r'));
    }

    #[test]
    fn format_notification_strips_carriage_returns() {
        // \r in content could submit arbitrary commands to Claude's PTY
        let msg = format_notification(0, None, "safe\rpwned\rtext");
        let inner = &msg[..msg.len() - 1]; // everything before trailing \r
        assert!(!inner.contains('\r'), "\\r in content must be sanitized");
        assert!(msg.ends_with('\r'), "trailing submit \\r must remain");
        assert!(msg.contains("safe"));
        assert!(msg.contains("pwned"));
    }

    #[test]
    fn format_notification_strips_all_control_chars() {
        let msg = format_notification(0, None, "hello\x00world\x1b[31m\ttabs");
        let inner = &msg[..msg.len() - 1];
        assert!(
            !inner.chars().any(|c| c.is_control()),
            "no control chars should survive in content"
        );
    }

    #[test]
    fn inject_into_missing_pane_returns_error() {
        let mut panes = PaneManager::new();
        let result = inject_into_pane(&mut panes, 0, "test");
        assert!(result.is_err());
    }
}
