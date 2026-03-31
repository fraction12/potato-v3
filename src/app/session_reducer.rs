//! Pure session-state reducer for [`AgentEvent`] → [`SessionState`] transitions.
//!
//! Extracting this logic from `main.rs` makes it unit-testable without a
//! terminal, PTY, or async runtime.
//!
//! # Design
//!
//! [`apply_event`] is the single entry-point: it takes a mutable reference to
//! [`SessionState`] and an [`AgentEvent`] and mutates the state in place.
//! It is intentionally side-effect-free (no I/O, no time — callers supply
//! `now` explicitly so tests can inject a fixed timestamp).

use chrono::{DateTime, Utc};

use crate::app::state::{
    AgentStatus, MessageRole, PendingApprovalSession, SessionState, ToolCallRecord, TranscriptEntry,
};
use crate::events::{AgentEvent, UsageInfo};

// ── Public API ────────────────────────────────────────────────────────────────

/// Apply a single [`AgentEvent`] to [`SessionState`], updating it in-place.
///
/// `now` is the wall-clock instant to use for any new timestamps; pass
/// `Utc::now()` in production and a fixed value in tests.
pub fn apply_event(session: &mut SessionState, event: AgentEvent, now: DateTime<Utc>) {
    match event {
        // ── Text streaming ────────────────────────────────────────────────────
        AgentEvent::TextDelta { text } => {
            // Append to the last assistant entry if one exists; otherwise open a new one.
            match session.transcript.last_mut() {
                Some(e) if e.role == MessageRole::Assistant => {
                    e.content.push_str(&text);
                }
                _ => {
                    let seq = session.next_turn_seq;
                    session.next_turn_seq = seq.wrapping_add(1);
                    session.active_turn_seq = Some(seq);
                    session.transcript.push(TranscriptEntry {
                        role: MessageRole::Assistant,
                        content: text,
                        timestamp: now,
                        tool_call: None,
                        turn_seq: seq,
                    });
                }
            }
            session.status = AgentStatus::Thinking;
        }

        AgentEvent::TextDone { full_text } => {
            // Patch the assistant entry that was being streamed, identified by
            // active_turn_seq. Falls back to the last assistant entry if no
            // active seq is set (T-874).
            let target_seq = session.active_turn_seq;
            let entry = if let Some(seq) = target_seq {
                session
                    .transcript
                    .iter_mut()
                    .rev()
                    .find(|e| e.role == MessageRole::Assistant && e.turn_seq == seq)
            } else {
                session
                    .transcript
                    .iter_mut()
                    .rev()
                    .find(|e| e.role == MessageRole::Assistant)
            };
            if let Some(e) = entry {
                e.content = full_text;
            }
            session.active_turn_seq = None;
        }

        // ── Tool lifecycle ────────────────────────────────────────────────────
        AgentEvent::ToolStart { id, name, input } => {
            session.status = AgentStatus::RunningTool { name: name.clone() };
            session.tool_calls.push(ToolCallRecord {
                id,
                name,
                input,
                output: None,
                started_at: now,
                duration_ms: None,
                success: None,
            });
        }

        AgentEvent::ToolDone {
            id,
            output,
            duration_ms,
            success,
        } => {
            if let Some(tc) = session.tool_calls.iter_mut().find(|t| t.id == id) {
                tc.output = Some(output);
                tc.duration_ms = Some(duration_ms);
                tc.success = Some(success);
            }
            // Tool finished → agent resumes thinking.
            session.status = AgentStatus::Thinking;
        }

        AgentEvent::ToolError { id, error } => {
            if let Some(tc) = session.tool_calls.iter_mut().find(|t| t.id == id) {
                tc.output = Some(error.clone());
                tc.success = Some(false);
                tc.duration_ms = tc.duration_ms.or(Some(0)); // mark as settled
            }
            // Tool errored → back to thinking so the agent can continue.
            session.status = AgentStatus::Thinking;
        }

        // ── Approval flow ─────────────────────────────────────────────────────
        AgentEvent::ApprovalRequired {
            tool_id,
            tool_name,
            input,
        } => {
            session.status = AgentStatus::WaitingApproval {
                tool_name: tool_name.clone(),
            };
            session.approval_pending = Some(PendingApprovalSession {
                tool_id,
                tool_name,
                input,
            });
        }

        AgentEvent::ApprovalDecision {
            tool_id: _,
            approved,
        } => {
            // The UI decision has been sent to the agent; clear the pending prompt.
            session.approval_pending = None;
            if approved {
                // Resume thinking while the agent processes the approved tool.
                session.status = AgentStatus::Thinking;
            } else {
                // Denial — agent returns to idle.
                session.status = AgentStatus::Idle;
            }
        }

        // ── Turn / session lifecycle ──────────────────────────────────────────
        AgentEvent::TurnStart => {
            session.status = AgentStatus::Thinking;
        }

        AgentEvent::TurnDone { usage } => {
            session.status = AgentStatus::Idle;
            if let Some(u) = usage {
                session.metrics.input_tokens += u.input_tokens;
                session.metrics.output_tokens += u.output_tokens;
                if let Some(cost) = u.cost_usd {
                    session.metrics.total_cost_usd += cost;
                }
                session.metrics.turn_count += 1;
            } else {
                session.metrics.turn_count += 1;
            }
        }

        AgentEvent::SessionBound { agent_session_id } => {
            // Store the Claude-native session id for use with --resume on the next turn.
            session.claude_session_id = Some(agent_session_id.clone());
            session.session_id = agent_session_id;
        }

        AgentEvent::AgentStarted { .. } => {
            session.status = AgentStatus::Thinking;
        }

        AgentEvent::AgentExited { exit_code } => {
            session.status = AgentStatus::Exited { code: exit_code };
        }

        // ── Diagnostics ───────────────────────────────────────────────────────
        AgentEvent::Error { message } => {
            session.status = AgentStatus::Error {
                message: message.clone(),
            };
            session.transcript.push(TranscriptEntry {
                role: MessageRole::System,
                content: format!("Error: {}", message),
                timestamp: now,
                tool_call: None,
                turn_seq: 0,
            });
        }

        AgentEvent::Warning { .. } | AgentEvent::Raw { .. } => {
            // Ignored at the session-state level; callers handle logging.
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::state::{AgentStatus, SessionState};
    use crate::events::{AgentEvent, UsageInfo};
    use chrono::{TimeZone, Utc};
    use serde_json::json;

    /// A fixed "now" timestamp so tests are deterministic.
    fn t0() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap()
    }

    fn fresh_session() -> SessionState {
        SessionState::new("sess-001", "claude")
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Dashboard → session transition (tested in state.rs; here we test
    // that a freshly-created SessionState has the expected initial shape)
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn new_session_starts_in_starting_status() {
        let s = fresh_session();
        assert_eq!(s.status, AgentStatus::Starting);
        assert!(s.transcript.is_empty());
        assert!(s.tool_calls.is_empty());
        assert!(s.approval_pending.is_none());
    }

    // ─────────────────────────────────────────────────────────────────────────
    // SessionBound
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn session_bound_replaces_session_id() {
        let mut s = fresh_session();
        assert_eq!(s.session_id, "sess-001");

        apply_event(
            &mut s,
            AgentEvent::SessionBound {
                agent_session_id: "native-abc-123".into(),
            },
            t0(),
        );

        assert_eq!(
            s.session_id, "native-abc-123",
            "SessionBound should overwrite the local placeholder id with the native agent session id"
        );
    }

    #[test]
    fn session_bound_populates_claude_session_id() {
        let mut s = fresh_session();
        assert!(
            s.claude_session_id.is_none(),
            "claude_session_id should start as None"
        );

        apply_event(
            &mut s,
            AgentEvent::SessionBound {
                agent_session_id: "claude-sess-xyz".into(),
            },
            t0(),
        );

        assert_eq!(
            s.claude_session_id.as_deref(),
            Some("claude-sess-xyz"),
            "SessionBound should store the native session id in claude_session_id for --resume",
        );
    }

    #[test]
    fn session_bound_updates_claude_session_id_on_subsequent_turns() {
        let mut s = fresh_session();
        apply_event(
            &mut s,
            AgentEvent::SessionBound {
                agent_session_id: "id-first".into(),
            },
            t0(),
        );
        apply_event(
            &mut s,
            AgentEvent::SessionBound {
                agent_session_id: "id-second".into(),
            },
            t0(),
        );
        assert_eq!(s.claude_session_id.as_deref(), Some("id-second"));
    }

    #[test]
    fn session_bound_does_not_clear_transcript() {
        let mut s = fresh_session();
        // Seed a transcript entry.
        apply_event(
            &mut s,
            AgentEvent::TextDelta {
                text: "hello".into(),
            },
            t0(),
        );
        apply_event(
            &mut s,
            AgentEvent::SessionBound {
                agent_session_id: "x".into(),
            },
            t0(),
        );
        assert_eq!(
            s.transcript.len(),
            1,
            "SessionBound must not clear transcript"
        );
    }

    // ─────────────────────────────────────────────────────────────────────────
    // TextDelta
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn text_delta_creates_assistant_entry_when_transcript_empty() {
        let mut s = fresh_session();
        apply_event(
            &mut s,
            AgentEvent::TextDelta {
                text: "Hello".into(),
            },
            t0(),
        );

        assert_eq!(s.transcript.len(), 1);
        let entry = &s.transcript[0];
        assert_eq!(entry.role, MessageRole::Assistant);
        assert_eq!(entry.content, "Hello");
        assert_eq!(entry.timestamp, t0());
    }

    #[test]
    fn text_delta_appends_to_existing_assistant_entry() {
        let mut s = fresh_session();
        apply_event(
            &mut s,
            AgentEvent::TextDelta {
                text: "Hello ".into(),
            },
            t0(),
        );
        apply_event(
            &mut s,
            AgentEvent::TextDelta {
                text: "world".into(),
            },
            t0(),
        );
        apply_event(&mut s, AgentEvent::TextDelta { text: "!".into() }, t0());

        assert_eq!(
            s.transcript.len(),
            1,
            "all deltas should merge into one entry"
        );
        assert_eq!(s.transcript[0].content, "Hello world!");
    }

    #[test]
    fn text_delta_opens_new_entry_after_user_message() {
        let mut s = fresh_session();
        // Simulate a user entry pushed externally.
        s.transcript.push(TranscriptEntry::user("What time is it?"));
        apply_event(
            &mut s,
            AgentEvent::TextDelta {
                text: "It's noon.".into(),
            },
            t0(),
        );

        assert_eq!(s.transcript.len(), 2);
        assert_eq!(s.transcript[1].role, MessageRole::Assistant);
        assert_eq!(s.transcript[1].content, "It's noon.");
    }

    #[test]
    fn text_delta_sets_status_to_thinking() {
        let mut s = fresh_session();
        apply_event(&mut s, AgentEvent::TextDelta { text: "...".into() }, t0());
        assert_eq!(s.status, AgentStatus::Thinking);
    }

    #[test]
    fn text_done_patches_last_assistant_entry() {
        let mut s = fresh_session();
        // Stream a few deltas.
        apply_event(&mut s, AgentEvent::TextDelta { text: "par".into() }, t0());
        apply_event(
            &mut s,
            AgentEvent::TextDelta {
                text: "tial".into(),
            },
            t0(),
        );
        // TextDone delivers authoritative full text.
        apply_event(
            &mut s,
            AgentEvent::TextDone {
                full_text: "complete answer".into(),
            },
            t0(),
        );

        assert_eq!(s.transcript.len(), 1);
        assert_eq!(s.transcript[0].content, "complete answer");
    }

    #[test]
    fn text_done_does_not_add_entry_if_no_assistant_entry() {
        let mut s = fresh_session();
        apply_event(
            &mut s,
            AgentEvent::TextDone {
                full_text: "orphan".into(),
            },
            t0(),
        );
        // Nothing to patch, transcript stays empty.
        assert!(s.transcript.is_empty());
    }

    // ─────────────────────────────────────────────────────────────────────────
    // ToolStart
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn tool_start_adds_record_and_sets_status() {
        let mut s = fresh_session();
        apply_event(
            &mut s,
            AgentEvent::ToolStart {
                id: "tool-1".into(),
                name: "read_file".into(),
                input: json!({ "path": "/tmp/foo.txt" }),
            },
            t0(),
        );

        assert_eq!(s.tool_calls.len(), 1);
        let tc = &s.tool_calls[0];
        assert_eq!(tc.id, "tool-1");
        assert_eq!(tc.name, "read_file");
        assert_eq!(tc.input, json!({ "path": "/tmp/foo.txt" }));
        assert_eq!(tc.started_at, t0());
        assert!(tc.output.is_none());
        assert!(tc.duration_ms.is_none());
        assert!(tc.success.is_none());
        assert_eq!(
            s.status,
            AgentStatus::RunningTool {
                name: "read_file".into()
            }
        );
    }

    #[test]
    fn tool_start_multiple_tools_stacked() {
        let mut s = fresh_session();
        apply_event(
            &mut s,
            AgentEvent::ToolStart {
                id: "t1".into(),
                name: "shell".into(),
                input: json!({}),
            },
            t0(),
        );
        apply_event(
            &mut s,
            AgentEvent::ToolStart {
                id: "t2".into(),
                name: "write_file".into(),
                input: json!({}),
            },
            t0(),
        );

        assert_eq!(s.tool_calls.len(), 2);
        // Status reflects the most recent start.
        assert_eq!(
            s.status,
            AgentStatus::RunningTool {
                name: "write_file".into()
            }
        );
    }

    // ─────────────────────────────────────────────────────────────────────────
    // ToolDone
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn tool_done_patches_matching_record() {
        let mut s = fresh_session();
        apply_event(
            &mut s,
            AgentEvent::ToolStart {
                id: "t1".into(),
                name: "shell".into(),
                input: json!({}),
            },
            t0(),
        );
        apply_event(
            &mut s,
            AgentEvent::ToolDone {
                id: "t1".into(),
                output: "ok".into(),
                duration_ms: 42,
                success: true,
            },
            t0(),
        );

        let tc = &s.tool_calls[0];
        assert_eq!(tc.output.as_deref(), Some("ok"));
        assert_eq!(tc.duration_ms, Some(42));
        assert_eq!(tc.success, Some(true));
    }

    #[test]
    fn tool_done_sets_status_to_thinking() {
        let mut s = fresh_session();
        apply_event(
            &mut s,
            AgentEvent::ToolStart {
                id: "t1".into(),
                name: "shell".into(),
                input: json!({}),
            },
            t0(),
        );
        apply_event(
            &mut s,
            AgentEvent::ToolDone {
                id: "t1".into(),
                output: "".into(),
                duration_ms: 0,
                success: true,
            },
            t0(),
        );
        assert_eq!(s.status, AgentStatus::Thinking);
    }

    #[test]
    fn tool_done_ignores_unknown_id() {
        let mut s = fresh_session();
        // No panic, no change.
        apply_event(
            &mut s,
            AgentEvent::ToolDone {
                id: "ghost".into(),
                output: "noop".into(),
                duration_ms: 0,
                success: true,
            },
            t0(),
        );
        assert!(s.tool_calls.is_empty());
    }

    // ─────────────────────────────────────────────────────────────────────────
    // ToolError
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn tool_error_marks_failure() {
        let mut s = fresh_session();
        apply_event(
            &mut s,
            AgentEvent::ToolStart {
                id: "t1".into(),
                name: "shell".into(),
                input: json!({}),
            },
            t0(),
        );
        apply_event(
            &mut s,
            AgentEvent::ToolError {
                id: "t1".into(),
                error: "permission denied".into(),
            },
            t0(),
        );

        let tc = &s.tool_calls[0];
        assert_eq!(tc.success, Some(false));
        assert_eq!(tc.output.as_deref(), Some("permission denied"));
    }

    #[test]
    fn tool_error_sets_status_to_thinking() {
        let mut s = fresh_session();
        apply_event(
            &mut s,
            AgentEvent::ToolStart {
                id: "t1".into(),
                name: "shell".into(),
                input: json!({}),
            },
            t0(),
        );
        apply_event(
            &mut s,
            AgentEvent::ToolError {
                id: "t1".into(),
                error: "boom".into(),
            },
            t0(),
        );
        assert_eq!(
            s.status,
            AgentStatus::Thinking,
            "after ToolError the agent should continue thinking (handle the error)"
        );
    }

    // ─────────────────────────────────────────────────────────────────────────
    // ApprovalRequired
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn approval_required_sets_waiting_status_and_pending() {
        let mut s = fresh_session();
        apply_event(
            &mut s,
            AgentEvent::ApprovalRequired {
                tool_id: "tool-42".into(),
                tool_name: "shell".into(),
                input: json!({ "cmd": "rm -rf /" }),
            },
            t0(),
        );

        assert_eq!(
            s.status,
            AgentStatus::WaitingApproval {
                tool_name: "shell".into()
            }
        );
        let pending = s
            .approval_pending
            .as_ref()
            .expect("should have pending approval");
        assert_eq!(pending.tool_id, "tool-42");
        assert_eq!(pending.tool_name, "shell");
        assert_eq!(pending.input, json!({ "cmd": "rm -rf /" }));
    }

    // ─────────────────────────────────────────────────────────────────────────
    // ApprovalDecision
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn approval_decision_approved_clears_pending() {
        let mut s = fresh_session();
        apply_event(
            &mut s,
            AgentEvent::ApprovalRequired {
                tool_id: "t-x".into(),
                tool_name: "shell".into(),
                input: json!({}),
            },
            t0(),
        );
        assert!(s.approval_pending.is_some());

        apply_event(
            &mut s,
            AgentEvent::ApprovalDecision {
                tool_id: "t-x".into(),
                approved: true,
            },
            t0(),
        );

        assert!(
            s.approval_pending.is_none(),
            "approval_pending should be cleared after decision"
        );
        assert_eq!(
            s.status,
            AgentStatus::Thinking,
            "status should return to Thinking after approval"
        );
    }

    #[test]
    fn approval_decision_denied_clears_pending() {
        let mut s = fresh_session();
        apply_event(
            &mut s,
            AgentEvent::ApprovalRequired {
                tool_id: "t-x".into(),
                tool_name: "shell".into(),
                input: json!({}),
            },
            t0(),
        );

        apply_event(
            &mut s,
            AgentEvent::ApprovalDecision {
                tool_id: "t-x".into(),
                approved: false,
            },
            t0(),
        );

        assert!(s.approval_pending.is_none());
    }

    // ─────────────────────────────────────────────────────────────────────────
    // TurnDone
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn turn_done_with_usage_updates_metrics_and_sets_idle() {
        let mut s = fresh_session();
        apply_event(
            &mut s,
            AgentEvent::TurnDone {
                usage: Some(UsageInfo {
                    input_tokens: 100,
                    output_tokens: 50,
                    cost_usd: Some(0.002),
                }),
            },
            t0(),
        );

        assert_eq!(s.status, AgentStatus::Idle);
        assert_eq!(s.metrics.input_tokens, 100);
        assert_eq!(s.metrics.output_tokens, 50);
        assert!((s.metrics.total_cost_usd - 0.002).abs() < 1e-9);
        assert_eq!(s.metrics.turn_count, 1);
    }

    #[test]
    fn turn_done_without_usage_increments_turn_count_only() {
        let mut s = fresh_session();
        apply_event(&mut s, AgentEvent::TurnDone { usage: None }, t0());

        assert_eq!(s.status, AgentStatus::Idle);
        assert_eq!(s.metrics.turn_count, 1);
        assert_eq!(s.metrics.input_tokens, 0);
    }

    #[test]
    fn turn_done_accumulates_across_multiple_turns() {
        let mut s = fresh_session();
        for i in 1u64..=3 {
            apply_event(
                &mut s,
                AgentEvent::TurnDone {
                    usage: Some(UsageInfo {
                        input_tokens: 10 * i,
                        output_tokens: 5 * i,
                        cost_usd: Some(0.001 * i as f64),
                    }),
                },
                t0(),
            );
        }
        assert_eq!(s.metrics.turn_count, 3);
        assert_eq!(s.metrics.input_tokens, 10 + 20 + 30);
        assert_eq!(s.metrics.output_tokens, 5 + 10 + 15);
        assert!((s.metrics.total_cost_usd - 0.006).abs() < 1e-9);
    }

    // ─────────────────────────────────────────────────────────────────────────
    // AgentExited
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn agent_exited_with_code_zero() {
        let mut s = fresh_session();
        apply_event(&mut s, AgentEvent::AgentExited { exit_code: Some(0) }, t0());
        assert_eq!(s.status, AgentStatus::Exited { code: Some(0) });
    }

    #[test]
    fn agent_exited_with_nonzero_code() {
        let mut s = fresh_session();
        apply_event(&mut s, AgentEvent::AgentExited { exit_code: Some(1) }, t0());
        assert_eq!(s.status, AgentStatus::Exited { code: Some(1) });
    }

    #[test]
    fn agent_exited_without_code() {
        let mut s = fresh_session();
        apply_event(&mut s, AgentEvent::AgentExited { exit_code: None }, t0());
        assert_eq!(s.status, AgentStatus::Exited { code: None });
    }

    #[test]
    fn agent_exited_does_not_clear_transcript() {
        let mut s = fresh_session();
        apply_event(
            &mut s,
            AgentEvent::TextDelta {
                text: "some output".into(),
            },
            t0(),
        );
        apply_event(&mut s, AgentEvent::AgentExited { exit_code: Some(0) }, t0());
        assert!(!s.transcript.is_empty(), "transcript should survive exit");
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Transcript ordering / timestamps
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn transcript_preserves_insertion_order() {
        let mut s = fresh_session();
        let t1 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 1).unwrap();
        let t2 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 2).unwrap();
        let t3 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 3).unwrap();

        // Manually push a user entry (as main.rs does when submitting input).
        s.transcript.push(TranscriptEntry {
            role: MessageRole::User,
            content: "hi".into(),
            timestamp: t1,
            tool_call: None,
            turn_seq: 0,
        });
        apply_event(
            &mut s,
            AgentEvent::TextDelta {
                text: "hello!".into(),
            },
            t2,
        );
        // TextDone comes later with the full text.
        apply_event(
            &mut s,
            AgentEvent::TextDone {
                full_text: "hello, world!".into(),
            },
            t3,
        );

        assert_eq!(s.transcript.len(), 2);
        assert_eq!(s.transcript[0].role, MessageRole::User);
        assert_eq!(s.transcript[0].timestamp, t1);
        assert_eq!(s.transcript[1].role, MessageRole::Assistant);
        // Timestamp was set when the first TextDelta opened the entry.
        assert_eq!(s.transcript[1].timestamp, t2);
        // Content was patched by TextDone.
        assert_eq!(s.transcript[1].content, "hello, world!");
    }

    #[test]
    fn transcript_entries_carry_correct_timestamps() {
        let mut s = fresh_session();
        let ta = Utc.with_ymd_and_hms(2025, 6, 15, 10, 0, 0).unwrap();
        let tb = Utc.with_ymd_and_hms(2025, 6, 15, 10, 0, 5).unwrap();

        apply_event(
            &mut s,
            AgentEvent::TextDelta {
                text: "first".into(),
            },
            ta,
        );
        // Simulate a turn boundary.
        apply_event(&mut s, AgentEvent::TurnDone { usage: None }, ta);
        // User message arrives (external push, simulated here).
        s.transcript.push(TranscriptEntry {
            role: MessageRole::User,
            content: "follow up".into(),
            timestamp: tb,
            tool_call: None,
            turn_seq: 0,
        });
        apply_event(
            &mut s,
            AgentEvent::TextDelta {
                text: "second".into(),
            },
            tb,
        );

        assert_eq!(s.transcript.len(), 3);
        assert_eq!(s.transcript[0].timestamp, ta);
        assert_eq!(s.transcript[2].timestamp, tb);
    }

    // ─────────────────────────────────────────────────────────────────────────
    // T-874: TextDone targets correct turn even if new turn started
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn text_done_patches_correct_turn_not_latest() {
        let mut s = fresh_session();
        let t1 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 1).unwrap();
        let t2 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 2).unwrap();

        // Turn 1: assistant starts streaming.
        apply_event(
            &mut s,
            AgentEvent::TextDelta {
                text: "partial".into(),
            },
            t1,
        );
        assert_eq!(s.transcript.len(), 1);
        assert_eq!(s.active_turn_seq, Some(0));

        // TextDone for turn 1 should patch the first entry.
        apply_event(
            &mut s,
            AgentEvent::TextDone {
                full_text: "complete turn 1".into(),
            },
            t2,
        );
        assert_eq!(s.transcript[0].content, "complete turn 1");
        assert_eq!(s.active_turn_seq, None);
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Full event sequence smoke test
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn full_happy_path_sequence() {
        let mut s = fresh_session();
        let now = t0();

        // 1. Agent binds to a session.
        apply_event(
            &mut s,
            AgentEvent::SessionBound {
                agent_session_id: "nat-001".into(),
            },
            now,
        );
        assert_eq!(s.session_id, "nat-001");

        // 2. Agent starts thinking (TurnStart).
        apply_event(&mut s, AgentEvent::TurnStart, now);
        assert_eq!(s.status, AgentStatus::Thinking);

        // 3. Streaming text arrives.
        apply_event(
            &mut s,
            AgentEvent::TextDelta {
                text: "I'll run a tool...".into(),
            },
            now,
        );
        assert_eq!(s.transcript.len(), 1);

        // 4. Tool starts.
        apply_event(
            &mut s,
            AgentEvent::ToolStart {
                id: "tool-1".into(),
                name: "shell".into(),
                input: json!({ "cmd": "ls" }),
            },
            now,
        );
        assert_eq!(s.tool_calls.len(), 1);
        assert_eq!(
            s.status,
            AgentStatus::RunningTool {
                name: "shell".into()
            }
        );

        // 5. Tool completes.
        apply_event(
            &mut s,
            AgentEvent::ToolDone {
                id: "tool-1".into(),
                output: "file.txt\n".into(),
                duration_ms: 10,
                success: true,
            },
            now,
        );
        assert_eq!(s.tool_calls[0].success, Some(true));
        assert_eq!(s.status, AgentStatus::Thinking);

        // 6. More streaming text.
        apply_event(
            &mut s,
            AgentEvent::TextDelta {
                text: " Done!".into(),
            },
            now,
        );

        // 7. Turn done.
        apply_event(
            &mut s,
            AgentEvent::TurnDone {
                usage: Some(UsageInfo {
                    input_tokens: 50,
                    output_tokens: 30,
                    cost_usd: Some(0.001),
                }),
            },
            now,
        );
        assert_eq!(s.status, AgentStatus::Idle);
        assert_eq!(s.metrics.turn_count, 1);
        assert_eq!(s.metrics.input_tokens, 50);
    }

    #[test]
    fn approval_flow_sequence() {
        let mut s = fresh_session();
        let now = t0();

        apply_event(&mut s, AgentEvent::TurnStart, now);
        apply_event(
            &mut s,
            AgentEvent::TextDelta {
                text: "About to run shell...".into(),
            },
            now,
        );
        apply_event(
            &mut s,
            AgentEvent::ApprovalRequired {
                tool_id: "t-rm".into(),
                tool_name: "shell".into(),
                input: json!({ "cmd": "rm important.txt" }),
            },
            now,
        );
        assert_eq!(
            s.status,
            AgentStatus::WaitingApproval {
                tool_name: "shell".into()
            }
        );
        assert!(s.approval_pending.is_some());

        // User approves.
        apply_event(
            &mut s,
            AgentEvent::ApprovalDecision {
                tool_id: "t-rm".into(),
                approved: true,
            },
            now,
        );
        assert!(s.approval_pending.is_none());
        assert_eq!(s.status, AgentStatus::Thinking);

        // Tool runs after approval.
        apply_event(
            &mut s,
            AgentEvent::ToolStart {
                id: "t-rm".into(),
                name: "shell".into(),
                input: json!({}),
            },
            now,
        );
        apply_event(
            &mut s,
            AgentEvent::ToolDone {
                id: "t-rm".into(),
                output: "removed".into(),
                duration_ms: 5,
                success: true,
            },
            now,
        );
        apply_event(&mut s, AgentEvent::TurnDone { usage: None }, now);
        assert_eq!(s.status, AgentStatus::Idle);
    }
}
