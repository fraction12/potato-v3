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
/// Uses bracketed paste mode (`\x1b[200~`...`\x1b[201~`) so terminal TUIs
/// (Claude Code, etc.) treat the entire block as a single paste rather than
/// interpreting each `\n` as Enter.
///
/// After the paste, a `\r` (Enter) is appended to submit the message.
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

    let body = format!(
        "[Potato: message from Pane {from_pane}{role_suffix}]\n\
         {content}\n\
         [/Potato]"
    );

    // Bracketed paste: \x1b[200~ ... \x1b[201~ then \r to submit.
    format!("\x1b[200~{body}\x1b[201~\r")
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
        assert!(msg.contains("[Potato: message from Pane 0]"));
        assert!(msg.contains("hello from pane 0"));
        assert!(msg.contains("[/Potato]"));
    }

    #[test]
    fn format_notification_with_role() {
        let msg = format_notification(1, Some("architect"), "the schema is ready");
        assert!(msg.contains("[Potato: message from Pane 1 (architect)]"));
        assert!(msg.contains("the schema is ready"));
    }

    #[test]
    fn format_notification_empty_role_is_omitted() {
        let msg = format_notification(0, Some(""), "test");
        assert!(msg.contains("[Potato: message from Pane 0]"));
        assert!(!msg.contains("()"));
    }

    #[test]
    fn format_notification_uses_bracketed_paste() {
        let msg = format_notification(0, None, "test content");
        // Must start with bracketed paste start sequence
        assert!(msg.starts_with("\x1b[200~"), "missing bracketed paste start");
        // Must contain bracketed paste end before the final \r
        assert!(msg.contains("\x1b[201~"), "missing bracketed paste end");
        // Must end with \r to submit
        assert!(msg.ends_with('\r'), "missing trailing carriage return");
    }

    #[test]
    fn format_notification_multiline_content() {
        let msg = format_notification(0, None, "line1\nline2\nline3");
        assert!(msg.contains("line1\nline2\nline3"));
        // Entire body is inside bracketed paste
        let start = "\x1b[200~";
        let end = "\x1b[201~";
        let body_start = msg.find(start).unwrap() + start.len();
        let body_end = msg.find(end).unwrap();
        let body = &msg[body_start..body_end];
        assert!(body.contains("[Potato:"));
        assert!(body.contains("[/Potato]"));
        assert!(body.contains("line1\nline2\nline3"));
    }

    #[test]
    fn inject_into_missing_pane_returns_error() {
        let mut panes = PaneManager::new();
        let result = inject_into_pane(&mut panes, 0, "test");
        assert!(result.is_err());
    }
}
