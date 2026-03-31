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

/// A pending injection that has been partially delivered (text written,
/// Enter not yet sent). The main loop should send `\r` after a short delay.
#[derive(Debug, Clone)]
pub struct PendingEnter {
    /// Stable pane ID (survives pane open/close).
    pub pane_id: u64,
    /// Tick count at which the text was written.
    pub written_at_tick: u64,
    /// How many ticks to wait before sending Enter.
    pub delay_ticks: u64,
}

/// Number of main-loop ticks to wait between writing message text and
/// sending `\r`. At ~20Hz tick rate (50ms), 5 ticks ≈ 250ms — enough for Claude's
/// Ink renderer to process the text before we submit.
pub const ENTER_DELAY_TICKS: u64 = 10; // ~500ms at 50ms/tick — gives Claude Ink time to settle

/// Format a structured message as a single-line notification nudge.
///
/// Instead of rendering the full message body into the PTY (which risks
/// `\n` being interpreted as Enter by the target agent's terminal), we
/// inject a short prompt telling the agent to call `potato_get_messages`
/// to read the actual content.
///
/// Format:
/// ```text
/// [Potato: Pane 0 (architect)] New [task] message: T-812: Wire up agent roster. Use potato_get_messages to read it.
/// ```
fn format_structured_nudge(prefix: &str, msg_type: &str, subject: &str) -> String {
    format!("{prefix} New [{msg_type}] message: {subject}. Use potato_get_messages to read it.")
}

/// Format a message notification for PTY injection.
///
/// For structured messages (JSON with type/subject), returns a single-line
/// nudge telling the agent to call `potato_get_messages` for the full content.
/// The PTY injection is just a notification — the MCP tool delivers the payload.
///
/// For legacy/freeform content, sanitizes control characters and returns a
/// single-line message.
///
/// Returns the text WITHOUT a trailing `\r`. The caller is responsible
/// for sending `\r` after a short delay to avoid the race condition where
/// Claude's Ink renderer swallows the Enter during active output.
pub fn format_notification(from_pane: u64, from_role: Option<&str>, content: &str) -> String {
    let role_suffix = from_role
        .filter(|r| !r.is_empty())
        .map(|r| format!(" ({r})"))
        .unwrap_or_default();

    let prefix = format!("[Potato: Pane {from_pane}{role_suffix}]");

    // Try to parse as structured JSON message — nudge agent to use MCP tool.
    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(content) {
        if let (Some(msg_type), Some(subject)) = (
            parsed.get("type").and_then(|v| v.as_str()),
            parsed.get("subject").and_then(|v| v.as_str()),
        ) {
            return format_structured_nudge(&prefix, msg_type, subject);
        }
    }

    // Fallback: legacy/freeform content — sanitize control chars.
    let sanitized: String = content
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let flat_content = sanitized.replace("  ", " ");
    format!("{prefix} {flat_content}")
}

/// Attempt to inject a formatted notification into a target pane's PTY.
///
/// Accepts a stable `pane_id` (u64) rather than a volatile Vec index, avoiding
/// index-vs-ID confusion (see T-862).
///
/// Returns `Ok(true)` if injected, `Ok(false)` if the target pane is in a
/// state that blocks injection (approval pending, not found, no PTY), and
/// `Err` on I/O failure.
pub fn inject_into_pane(
    panes: &mut PaneManager,
    target_pane_id: u64,
    text: &str,
) -> Result<bool, String> {
    let index = panes
        .find_by_pane_id(target_pane_id)
        .ok_or_else(|| format!("target pane id {target_pane_id} not found"))?;
    let pane = panes
        .get_mut(index)
        .ok_or_else(|| format!("target pane id {target_pane_id} not found"))?;

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
        assert!(
            !msg.ends_with('\r'),
            "should not include trailing Enter (deferred)"
        );
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
    fn format_notification_legacy_single_line_no_newlines() {
        let msg = format_notification(0, None, "test content");
        // Legacy (non-JSON) messages are single-line.
        assert!(!msg.contains('\n'), "legacy must be single-line");
        assert!(!msg.contains('\r'), "Enter is deferred — no \\r in text");
        assert!(msg.contains("[Potato:"));
        assert!(msg.contains("test content"));
    }

    #[test]
    fn format_notification_legacy_multiline_flattened() {
        let msg = format_notification(0, None, "line1\nline2\nline3");
        // Legacy messages flatten newlines.
        assert!(!msg.contains('\n'), "legacy newlines must be flattened");
        assert!(msg.contains("line1 line2 line3"));
    }

    #[test]
    fn format_notification_strips_carriage_returns() {
        let msg = format_notification(0, None, "safe\rpwned\rtext");
        assert!(!msg.contains('\r'), "\\r in content must be sanitized");
        assert!(msg.contains("safe"));
        assert!(msg.contains("pwned"));
    }

    #[test]
    fn format_notification_strips_all_control_chars() {
        let msg = format_notification(0, None, "hello\x00world\x1b[31m\ttabs");
        assert!(
            !msg.chars().any(|c| c.is_control()),
            "no control chars should survive in content"
        );
    }

    #[test]
    fn format_notification_structured_is_single_line_nudge() {
        let content = serde_json::json!({
            "type": "task",
            "subject": "T-812: Wire up agent roster",
            "body": {
                "summary": "ProfileLoader exists but is never called.",
                "files": ["src/config/profiles.rs", "src/app/state.rs"],
                "steps": ["Rename profiles.toml", "Feed into AppState"],
                "context": "Blocked on config module refactor."
            }
        })
        .to_string();
        let msg = format_notification(0, Some("architect"), &content);
        assert_eq!(
            msg,
            "[Potato: Pane 0 (architect)] New [task] message: T-812: Wire up agent roster. Use potato_get_messages to read it."
        );
        assert!(!msg.contains('\n'), "nudge must be single-line");
        assert!(!msg.contains('\r'), "no trailing Enter");
    }

    #[test]
    fn format_notification_structured_nudge_various_types() {
        for (msg_type, subject) in [
            ("question", "Which DB migration tool?"),
            ("status", "Progress update"),
            ("result", "Completed refactor"),
        ] {
            let content = serde_json::json!({
                "type": msg_type,
                "subject": subject,
                "body": { "summary": "details here" }
            })
            .to_string();
            let msg = format_notification(1, Some("implementer"), &content);
            assert!(
                msg.contains(&format!("New [{msg_type}] message: {subject}")),
                "should contain type and subject"
            );
            assert!(
                msg.contains("potato_get_messages"),
                "should tell agent to use MCP tool"
            );
            assert!(!msg.contains('\n'), "must be single-line");
        }
    }

    #[test]
    fn format_notification_structured_nudge_minimal_body() {
        let content = serde_json::json!({
            "type": "ping",
            "subject": "Are you there?",
            "body": {}
        })
        .to_string();
        let msg = format_notification(0, None, &content);
        assert_eq!(
            msg,
            "[Potato: Pane 0] New [ping] message: Are you there?. Use potato_get_messages to read it."
        );
    }

    #[test]
    fn format_notification_structured_body_not_leaked() {
        let content = serde_json::json!({
            "type": "task",
            "subject": "Test",
            "body": {
                "summary": "secret details",
                "steps": ["step 1"],
                "files": ["a.rs"],
                "context": "extra context"
            }
        })
        .to_string();
        let msg = format_notification(0, None, &content);
        assert!(
            !msg.contains("secret details"),
            "body should not appear in nudge"
        );
        assert!(!msg.contains("step 1"), "steps should not appear in nudge");
        assert!(!msg.contains("a.rs"), "files should not appear in nudge");
        assert!(
            !msg.contains("extra context"),
            "context should not appear in nudge"
        );
    }

    #[test]
    fn format_notification_legacy_fallback() {
        // Non-JSON content should fall back to the old sanitize behavior.
        let msg = format_notification(0, Some("worker"), "plain text message");
        assert_eq!(msg, "[Potato: Pane 0 (worker)] plain text message");
    }

    #[test]
    fn format_notification_malformed_json_fallback() {
        // JSON that doesn't have type/subject should fall back.
        let content = serde_json::json!({"foo": "bar"}).to_string();
        let msg = format_notification(0, None, &content);
        assert!(msg.starts_with("[Potato: Pane 0]"));
        assert!(msg.contains("foo"));
    }

    #[test]
    fn enter_delay_ticks_is_positive() {
        const { assert!(ENTER_DELAY_TICKS > 0) };
    }

    #[test]
    fn inject_into_missing_pane_returns_error() {
        let mut panes = PaneManager::new();
        let result = inject_into_pane(&mut panes, 99, "test");
        assert!(result.is_err());
    }

    #[test]
    fn inject_into_pane_resolves_id_not_index() {
        let mut panes = PaneManager::new();
        panes.open("sess-1", "claude"); // id=0
        panes.open("sess-2", "claude"); // id=1
        // Close first pane — index 0 now holds the pane with id=1.
        panes.close(0);
        // Injecting by pane_id=1 should find the pane even though its index shifted.
        let result = inject_into_pane(&mut panes, 1, "test");
        // Will be Err because no PTY, but importantly it should NOT be "not found".
        assert!(
            result.is_err() && result.unwrap_err().contains("no PTY"),
            "should find pane by ID, not index"
        );
    }
}
