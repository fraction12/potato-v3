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
/// Format a message notification for PTY injection.
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

    // Try to parse as structured JSON message.
    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(content) {
        if let (Some(msg_type), Some(subject)) = (
            parsed.get("type").and_then(|v| v.as_str()),
            parsed.get("subject").and_then(|v| v.as_str()),
        ) {
            let mut suffix_parts = Vec::new();
            if let Some(body) = parsed.get("body") {
                if let Some(steps) = body.get("steps").and_then(|v| v.as_array()) {
                    suffix_parts.push(format!("{} steps", steps.len()));
                }
                if let Some(files) = body.get("files").and_then(|v| v.as_array()) {
                    suffix_parts.push(format!("{} files", files.len()));
                }
            }
            let suffix = if suffix_parts.is_empty() {
                String::new()
            } else {
                format!(" | {}", suffix_parts.join(", "))
            };
            return format!("{prefix} [{msg_type}] {subject}{suffix}");
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
    fn format_notification_single_line_no_newlines() {
        let msg = format_notification(0, None, "test content");
        assert!(
            !msg.contains('\n'),
            "must be single-line for Claude Code raw mode"
        );
        assert!(!msg.contains('\r'), "Enter is deferred — no \\r in text");
        assert!(msg.contains("[Potato:"));
        assert!(msg.contains("test content"));
    }

    #[test]
    fn format_notification_multiline_flattened() {
        let msg = format_notification(0, None, "line1\nline2\nline3");
        assert!(!msg.contains('\n'), "newlines must be flattened");
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
    fn format_notification_structured_with_steps_and_files() {
        let content = serde_json::json!({
            "type": "task",
            "subject": "T-812: Wire up agent roster",
            "body": {
                "summary": "ProfileLoader exists but is never called.",
                "files": ["src/config/profiles.rs", "src/app/state.rs", "src/ui/overlays/agent_picker.rs"],
                "steps": ["Rename profiles.toml", "Feed into AppState", "Update picker", "Test"]
            }
        }).to_string();
        let msg = format_notification(0, Some("architect"), &content);
        assert_eq!(
            msg,
            "[Potato: Pane 0 (architect)] [task] T-812: Wire up agent roster | 4 steps, 3 files"
        );
    }

    #[test]
    fn format_notification_structured_no_steps_or_files() {
        let content = serde_json::json!({
            "type": "question",
            "subject": "Which DB migration tool?",
            "body": { "summary": "Should we use refinery or sqlx-migrate?" }
        })
        .to_string();
        let msg = format_notification(1, Some("implementer"), &content);
        assert_eq!(
            msg,
            "[Potato: Pane 1 (implementer)] [question] Which DB migration tool?"
        );
    }

    #[test]
    fn format_notification_structured_steps_only() {
        let content = serde_json::json!({
            "type": "status",
            "subject": "Progress update",
            "body": { "summary": "Done with step 1", "steps": ["step 1", "step 2"] }
        })
        .to_string();
        let msg = format_notification(0, None, &content);
        assert_eq!(msg, "[Potato: Pane 0] [status] Progress update | 2 steps");
    }

    #[test]
    fn format_notification_structured_files_only() {
        let content = serde_json::json!({
            "type": "result",
            "subject": "Completed refactor",
            "body": { "summary": "Refactored X", "files": ["a.rs"] }
        })
        .to_string();
        let msg = format_notification(0, None, &content);
        assert_eq!(
            msg,
            "[Potato: Pane 0] [result] Completed refactor | 1 files"
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
