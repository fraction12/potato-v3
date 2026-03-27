//! Claude Code adapter — spawns `claude --output-format stream-json --verbose`
//! and parses the NDJSON output into canonical [`AgentEvent`]s.

use std::path::PathBuf;

use tokio::process::Command;
use tracing::warn;

use super::{AdapterCapabilities, AdapterConfig, AgentAdapter};
use crate::events::{AgentEvent, UsageInfo};

// ── ClaudeAdapter ─────────────────────────────────────────────────────────────

/// Adapter for the Claude Code CLI (`claude`).
pub struct ClaudeAdapter;

impl AgentAdapter for ClaudeAdapter {
    fn name(&self) -> &str {
        "claude"
    }

    /// Locate the `claude` binary via PATH or common install locations.
    fn detect(&self) -> Option<PathBuf> {
        // Try PATH first.
        if let Ok(p) = which::which("claude") {
            return Some(p);
        }
        // Common macOS/Linux install paths.
        let mut candidates: Vec<PathBuf> = vec![
            PathBuf::from("/usr/local/bin/claude"),
            PathBuf::from("/usr/bin/claude"),
        ];
        if let Some(home) = dirs::home_dir() {
            candidates.push(home.join(".local/bin/claude"));
        }
        for c in &candidates {
            if c.exists() {
                return Some(c.clone());
            }
        }
        None
    }

    fn capabilities(&self) -> AdapterCapabilities {
        AdapterCapabilities {
            structured_output: true,
            // Claude supports `--resume <session_id>` for session continuation.
            session_resumable: true,
            approval_intercept: true,
            tool_events: true,
        }
    }

    /// Build the Claude CLI command.
    ///
    /// Produces: `claude --print --output-format stream-json --verbose [--resume id] [--model m] [flags…]`
    ///
    /// `--print` is required by the Claude CLI when using `--output-format stream-json`.
    /// The prompt is NOT passed as a CLI arg — it must be piped via stdin.
    fn build_command(&self, config: &AdapterConfig) -> Command {
        let binary = self.detect().unwrap_or_else(|| PathBuf::from("claude"));
        let mut cmd = Command::new(binary);

        cmd.current_dir(&config.working_dir);
        // --print is required for stream-json output format.
        // Prompt is passed via stdin, not as a CLI argument.
        cmd.args(["--print", "--output-format", "stream-json", "--verbose"]);

        if let Some(ref session_id) = config.resume_session_id {
            cmd.args(["--resume", session_id]);
        }

        if let Some(ref model) = config.model {
            cmd.args(["--model", model]);
        }

        for flag in &config.extra_flags {
            cmd.arg(flag);
        }

        cmd
    }

    /// Parse a single NDJSON line from Claude's `stream-json` output.
    ///
    /// Claude emits lines like:
    /// - `{"type":"system","subtype":"init","session_id":"…","tools":[…]}`
    /// - `{"type":"assistant","message":{"content":[{"type":"text","text":"…"}]}}`
    /// - `{"type":"assistant","message":{"content":[{"type":"tool_use","id":"…","name":"…","input":{}}]}}`
    /// - `{"type":"result","subtype":"success","usage":{"input_tokens":n,"output_tokens":n}}`
    fn parse_line(&self, line: &str) -> Vec<AgentEvent> {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            return vec![];
        }

        let value: serde_json::Value = match serde_json::from_str(trimmed) {
            Ok(v) => v,
            Err(_) => {
                return vec![AgentEvent::Raw { payload: trimmed.to_string() }];
            }
        };

        let msg_type = value["type"].as_str().unwrap_or("");

        match msg_type {
            // ── System events ─────────────────────────────────────────────────
            "system" => {
                match value["subtype"].as_str() {
                    // hook_started and hook_response are internal lifecycle events — silently ignore.
                    Some("hook_started") | Some("hook_response") => vec![],
                    // init → bind the agent session id.
                    Some("init") => {
                        if let Some(session_id) = value["session_id"].as_str() {
                            vec![AgentEvent::SessionBound { agent_session_id: session_id.to_string() }]
                        } else {
                            vec![AgentEvent::Raw { payload: trimmed.to_string() }]
                        }
                    }
                    _ => vec![AgentEvent::Raw { payload: trimmed.to_string() }],
                }
            }

            // ── Assistant message (text or tool_use) ─────────────────────────
            "assistant" => {
                let mut events = vec![];
                if let Some(content_arr) = value["message"]["content"].as_array() {
                    for item in content_arr {
                        match item["type"].as_str() {
                            Some("text") => {
                                if let Some(text) = item["text"].as_str() {
                                    if !text.is_empty() {
                                        events.push(AgentEvent::TextDelta { text: text.to_string() });
                                    }
                                }
                            }
                            Some("tool_use") => {
                                let id = item["id"].as_str().unwrap_or_else(|| {
                                    warn!("claude adapter: tool_use missing 'id' field");
                                    ""
                                }).to_string();
                                let name = item["name"].as_str().unwrap_or_else(|| {
                                    warn!("claude adapter: tool_use missing 'name' field");
                                    ""
                                }).to_string();
                                let input = item["input"].clone();
                                events.push(AgentEvent::ToolStart { id, name, input });
                            }
                            _ => {}
                        }
                    }
                }
                if events.is_empty() {
                    vec![AgentEvent::Raw { payload: trimmed.to_string() }]
                } else {
                    events
                }
            }

            // ── Tool result (from Claude's tool execution loop) ───────────────
            "tool_result" => {
                let id = value["tool_use_id"].as_str().unwrap_or("").to_string();
                let output = value["content"].as_str().unwrap_or("").to_string();
                vec![AgentEvent::ToolDone {
                    id,
                    output,
                    duration_ms: 0,
                    success: true,
                }]
            }

            // ── Turn result ──────────────────────────────────────────────────
            "result" => {
                let subtype = value["subtype"].as_str().unwrap_or("");
                let usage = parse_usage(&value);

                if subtype == "success" || subtype == "error_max_turns" {
                    // Emit a TextDone if there is a result message
                    let mut events = vec![];
                    if let Some(result_text) = value["result"].as_str() {
                        if !result_text.is_empty() {
                            events.push(AgentEvent::TextDone { full_text: result_text.to_string() });
                        }
                    }
                    events.push(AgentEvent::TurnDone { usage });
                    events
                } else if subtype == "error" {
                    let msg = value["error"].as_str().unwrap_or("unknown error").to_string();
                    vec![
                        AgentEvent::Error { message: msg },
                        AgentEvent::TurnDone { usage },
                    ]
                } else {
                    vec![AgentEvent::TurnDone { usage }]
                }
            }

            // ── User message echo (ignore) ────────────────────────────────────
            "user" => vec![],

            // ── Rate limit events (ignore) ────────────────────────────────────
            "rate_limit_event" => vec![],

            // ── Unknown / passthrough ─────────────────────────────────────────
            _ => {
                vec![AgentEvent::Raw { payload: trimmed.to_string() }]
            }
        }
    }

    /// Format user input for Claude's stdin: raw text followed by a newline.
    fn format_user_input(&self, text: &str) -> String {
        format!("{text}\n")
    }

    /// Approval decision: `"y\n"` for approved, `"n\n"` for denied.
    fn format_approval(&self, approved: bool) -> Option<String> {
        if approved { Some("y\n".to_string()) } else { Some("n\n".to_string()) }
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Extract usage information from a result event, if present.
fn parse_usage(value: &serde_json::Value) -> Option<UsageInfo> {
    let usage = &value["usage"];
    if usage.is_null() {
        return None;
    }
    let input_tokens = usage["input_tokens"].as_u64().unwrap_or(0);
    let output_tokens = usage["output_tokens"].as_u64().unwrap_or(0);
    let cost_usd = value["cost_usd"].as_f64();
    Some(UsageInfo { input_tokens, output_tokens, cost_usd })
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;

    fn adapter() -> ClaudeAdapter { ClaudeAdapter }

    // ── parse_line: empty / whitespace ────────────────────────────────────────

    #[test]
    fn parse_empty_line() {
        assert!(adapter().parse_line("").is_empty());
        assert!(adapter().parse_line("   ").is_empty());
        assert!(adapter().parse_line("\t\n").is_empty());
    }

    // ── parse_line: invalid JSON → Raw ────────────────────────────────────────

    #[test]
    fn parse_invalid_json_returns_raw() {
        let events = adapter().parse_line("not json at all");
        assert_eq!(events.len(), 1);
        assert!(matches!(&events[0], AgentEvent::Raw { payload } if payload == "not json at all"));
    }

    #[test]
    fn parse_truncated_json_returns_raw() {
        let events = adapter().parse_line(r#"{"type":"assistant","message":"#);
        assert_eq!(events.len(), 1);
        assert!(matches!(&events[0], AgentEvent::Raw { .. }));
    }

    #[test]
    fn parse_json_array_not_object_returns_raw() {
        // Valid JSON but not an object — treated as unknown type → Raw
        let events = adapter().parse_line(r#"[1,2,3]"#);
        assert_eq!(events.len(), 1);
        assert!(matches!(&events[0], AgentEvent::Raw { .. }));
    }

    // ── parse_line: system/init → SessionBound ────────────────────────────────

    #[test]
    fn parse_system_init_binds_session_id() {
        let line = r#"{"type":"system","subtype":"init","session_id":"s-abc","tools":[]}"#;
        let events = adapter().parse_line(line);
        assert_eq!(events.len(), 1);
        assert!(
            matches!(&events[0], AgentEvent::SessionBound { agent_session_id } if agent_session_id == "s-abc"),
            "expected SessionBound with s-abc, got {:?}", events[0]
        );
    }

    #[test]
    fn parse_system_init_preserves_full_session_id() {
        let line = r#"{"type":"system","subtype":"init","session_id":"ses-0000-ffff-1234"}"#;
        let events = adapter().parse_line(line);
        assert_eq!(events.len(), 1);
        if let AgentEvent::SessionBound { agent_session_id } = &events[0] {
            assert_eq!(agent_session_id, "ses-0000-ffff-1234");
        } else {
            panic!("expected SessionBound, got {:?}", events[0]);
        }
    }

    #[test]
    fn parse_system_init_without_session_id_returns_raw() {
        // If the init line is missing session_id, fall back to Raw so nothing is silently lost.
        let line = r#"{"type":"system","subtype":"init","tools":[]}"#;
        let events = adapter().parse_line(line);
        assert_eq!(events.len(), 1);
        assert!(
            matches!(&events[0], AgentEvent::Raw { .. }),
            "expected Raw when session_id is absent, got {:?}", events[0]
        );
    }

    #[test]
    fn parse_system_other_subtype_returns_raw() {
        let line = r#"{"type":"system","subtype":"shutdown"}"#;
        let events = adapter().parse_line(line);
        assert_eq!(events.len(), 1);
        assert!(matches!(&events[0], AgentEvent::Raw { .. }));
    }

    #[test]
    fn parse_system_hook_started_returns_empty() {
        let line = r#"{"type":"system","subtype":"hook_started","hook_id":"h1"}"#;
        let events = adapter().parse_line(line);
        assert!(events.is_empty(), "hook_started should be silently ignored, got {:?}", events);
    }

    #[test]
    fn parse_system_hook_response_returns_empty() {
        let line = r#"{"type":"system","subtype":"hook_response","hook_id":"h1","response":{}}"#;
        let events = adapter().parse_line(line);
        assert!(events.is_empty(), "hook_response should be silently ignored, got {:?}", events);
    }

    #[test]
    fn parse_rate_limit_event_returns_empty() {
        let line = r#"{"type":"rate_limit_event","retry_after_ms":5000}"#;
        let events = adapter().parse_line(line);
        assert!(events.is_empty(), "rate_limit_event should be silently ignored, got {:?}", events);
    }

    // ── parse_line: assistant text → TextDelta ────────────────────────────────

    #[test]
    fn parse_text_delta_basic() {
        let line = r#"{"type":"assistant","message":{"content":[{"type":"text","text":"Hello!"}]}}"#;
        let events = adapter().parse_line(line);
        assert_eq!(events.len(), 1);
        assert!(
            matches!(&events[0], AgentEvent::TextDelta { text } if text == "Hello!"),
            "expected TextDelta 'Hello!', got {:?}", events[0]
        );
    }

    #[test]
    fn parse_text_delta_multiline_text() {
        let line = r#"{"type":"assistant","message":{"content":[{"type":"text","text":"line1\nline2"}]}}"#;
        let events = adapter().parse_line(line);
        assert_eq!(events.len(), 1);
        if let AgentEvent::TextDelta { text } = &events[0] {
            assert_eq!(text, "line1\nline2");
        } else {
            panic!("expected TextDelta, got {:?}", events[0]);
        }
    }

    #[test]
    fn parse_text_delta_empty_text_skipped_produces_raw() {
        // Empty text items should be skipped; if no events result, we get Raw.
        let line = r#"{"type":"assistant","message":{"content":[{"type":"text","text":""}]}}"#;
        let events = adapter().parse_line(line);
        assert_eq!(events.len(), 1);
        assert!(
            matches!(&events[0], AgentEvent::Raw { .. }),
            "empty text should fall back to Raw, got {:?}", events[0]
        );
    }

    // ── parse_line: tool_use → ToolStart ──────────────────────────────────────

    #[test]
    fn parse_tool_use_basic() {
        let line = r#"{"type":"assistant","message":{"content":[{"type":"tool_use","id":"t1","name":"read_file","input":{"path":"/tmp/x"}}]}}"#;
        let events = adapter().parse_line(line);
        assert_eq!(events.len(), 1);
        assert!(
            matches!(&events[0], AgentEvent::ToolStart { id, name, .. } if id == "t1" && name == "read_file"),
            "expected ToolStart t1/read_file, got {:?}", events[0]
        );
    }

    #[test]
    fn parse_tool_use_input_is_preserved() {
        let line = r#"{"type":"assistant","message":{"content":[{"type":"tool_use","id":"t2","name":"write_file","input":{"path":"/foo","content":"bar"}}]}}"#;
        let events = adapter().parse_line(line);
        assert_eq!(events.len(), 1);
        if let AgentEvent::ToolStart { id, name, input } = &events[0] {
            assert_eq!(id, "t2");
            assert_eq!(name, "write_file");
            assert_eq!(input["path"].as_str(), Some("/foo"));
            assert_eq!(input["content"].as_str(), Some("bar"));
        } else {
            panic!("expected ToolStart, got {:?}", events[0]);
        }
    }

    #[test]
    fn parse_tool_use_empty_input_is_object() {
        let line = r#"{"type":"assistant","message":{"content":[{"type":"tool_use","id":"t3","name":"list_dir","input":{}}]}}"#;
        let events = adapter().parse_line(line);
        assert_eq!(events.len(), 1);
        if let AgentEvent::ToolStart { input, .. } = &events[0] {
            assert!(input.is_object(), "input should be an object even when empty");
        } else {
            panic!("expected ToolStart, got {:?}", events[0]);
        }
    }

    // ── parse_line: mixed content (text + tool_use) ───────────────────────────

    #[test]
    fn parse_mixed_content_emits_text_and_tool() {
        let line = r#"{"type":"assistant","message":{"content":[
            {"type":"text","text":"I will read it"},
            {"type":"tool_use","id":"t4","name":"read_file","input":{"path":"/etc/hosts"}}
        ]}}"#;
        let events = adapter().parse_line(line);
        assert_eq!(events.len(), 2);
        assert!(events.iter().any(|e| matches!(e, AgentEvent::TextDelta { .. })));
        assert!(events.iter().any(|e| matches!(e, AgentEvent::ToolStart { .. })));
    }

    // ── parse_line: tool_result → ToolDone ───────────────────────────────────

    #[test]
    fn parse_tool_result_returns_tool_done() {
        let line = r#"{"type":"tool_result","tool_use_id":"t1","content":"file contents here"}"#;
        let events = adapter().parse_line(line);
        assert_eq!(events.len(), 1);
        assert!(
            matches!(&events[0], AgentEvent::ToolDone { id, output, success, .. }
                if id == "t1" && output == "file contents here" && *success),
            "expected ToolDone with correct id/output, got {:?}", events[0]
        );
    }

    // ── parse_line: result success with usage → TurnDone ─────────────────────

    #[test]
    fn parse_result_success_emits_text_done_and_turn_done() {
        let line = r#"{"type":"result","subtype":"success","result":"All done","usage":{"input_tokens":100,"output_tokens":50},"cost_usd":0.001}"#;
        let events = adapter().parse_line(line);
        assert!(events.iter().any(|e| matches!(e, AgentEvent::TextDone { .. })));
        assert!(events.iter().any(|e| matches!(e, AgentEvent::TurnDone { usage: Some(_) })));
    }

    #[test]
    fn parse_result_success_usage_values_correct() {
        let line = r#"{"type":"result","subtype":"success","result":"","usage":{"input_tokens":200,"output_tokens":75},"cost_usd":0.005}"#;
        let events = adapter().parse_line(line);
        let turn_done = events.iter().find(|e| matches!(e, AgentEvent::TurnDone { .. }))
            .expect("must have TurnDone");
        if let AgentEvent::TurnDone { usage: Some(u) } = turn_done {
            assert_eq!(u.input_tokens, 200);
            assert_eq!(u.output_tokens, 75);
            assert_eq!(u.cost_usd, Some(0.005));
        } else {
            panic!("expected TurnDone with Some(usage), got {:?}", turn_done);
        }
    }

    #[test]
    fn parse_result_success_no_usage_gives_turn_done_none() {
        let line = r#"{"type":"result","subtype":"success","result":""}"#;
        let events = adapter().parse_line(line);
        assert!(events.iter().any(|e| matches!(e, AgentEvent::TurnDone { usage: None })));
    }

    #[test]
    fn parse_result_success_empty_result_no_text_done() {
        // Empty result string → no TextDone emitted, just TurnDone
        let line = r#"{"type":"result","subtype":"success","result":"","usage":null}"#;
        let events = adapter().parse_line(line);
        assert!(!events.iter().any(|e| matches!(e, AgentEvent::TextDone { .. })));
        assert!(events.iter().any(|e| matches!(e, AgentEvent::TurnDone { .. })));
    }

    #[test]
    fn parse_result_error_max_turns_emits_turn_done() {
        let line = r#"{"type":"result","subtype":"error_max_turns","result":"","usage":{"input_tokens":10,"output_tokens":5}}"#;
        let events = adapter().parse_line(line);
        assert!(events.iter().any(|e| matches!(e, AgentEvent::TurnDone { .. })));
    }

    #[test]
    fn parse_result_error_subtype_emits_error_and_turn_done() {
        let line = r#"{"type":"result","subtype":"error","error":"Something failed","usage":null}"#;
        let events = adapter().parse_line(line);
        assert!(
            events.iter().any(|e| matches!(e, AgentEvent::Error { message } if message == "Something failed")),
            "expected Error event with message, got {:?}", events
        );
        assert!(events.iter().any(|e| matches!(e, AgentEvent::TurnDone { .. })));
    }

    #[test]
    fn parse_result_unknown_subtype_emits_turn_done() {
        let line = r#"{"type":"result","subtype":"future_thing"}"#;
        let events = adapter().parse_line(line);
        assert!(events.iter().any(|e| matches!(e, AgentEvent::TurnDone { .. })));
    }

    // ── parse_line: user echo → empty ─────────────────────────────────────────

    #[test]
    fn parse_user_message_returns_empty() {
        let line = r#"{"type":"user","message":{"content":[{"type":"text","text":"hi"}]}}"#;
        let events = adapter().parse_line(line);
        assert!(events.is_empty(), "user echo should produce no events, got {:?}", events);
    }

    // ── parse_line: unknown type → Raw ────────────────────────────────────────

    #[test]
    fn parse_unknown_type_is_raw() {
        let line = r#"{"type":"debug","payload":"something"}"#;
        let events = adapter().parse_line(line);
        assert_eq!(events.len(), 1);
        assert!(matches!(&events[0], AgentEvent::Raw { .. }));
    }

    #[test]
    fn parse_unknown_type_raw_payload_preserved() {
        let line = r#"{"type":"metrics","data":{"latency":42}}"#;
        let events = adapter().parse_line(line);
        assert_eq!(events.len(), 1);
        if let AgentEvent::Raw { payload } = &events[0] {
            assert!(payload.contains("metrics"), "payload should contain original content");
        } else {
            panic!("expected Raw, got {:?}", events[0]);
        }
    }

    // ── format_user_input ─────────────────────────────────────────────────────

    #[test]
    fn format_user_input_appends_newline() {
        assert_eq!(adapter().format_user_input("hello world"), "hello world\n");
    }

    #[test]
    fn format_user_input_empty_string() {
        assert_eq!(adapter().format_user_input(""), "\n");
    }

    #[test]
    fn format_user_input_already_has_newline() {
        // Should still append — raw passthrough + newline
        assert_eq!(adapter().format_user_input("hi\n"), "hi\n\n");
    }

    #[test]
    fn format_user_input_multiline() {
        let result = adapter().format_user_input("line1\nline2");
        assert_eq!(result, "line1\nline2\n");
    }

    // ── format_approval ───────────────────────────────────────────────────────

    #[test]
    fn format_approval_yes() {
        assert_eq!(adapter().format_approval(true), Some("y\n".to_string()));
    }

    #[test]
    fn format_approval_no() {
        assert_eq!(adapter().format_approval(false), Some("n\n".to_string()));
    }

    // ── build_command: required flags ─────────────────────────────────────────

    /// Extract all args from a Command as a Vec<String>.
    /// Safety: clones all OsStr args via to_string_lossy.
    fn cmd_args(cmd: &Command) -> Vec<String> {
        cmd.as_std()
            .get_args()
            .map(|a: &OsStr| a.to_string_lossy().into_owned())
            .collect()
    }

    fn default_config() -> AdapterConfig {
        AdapterConfig {
            working_dir: std::path::PathBuf::from("/tmp"),
            model: None,
            resume_session_id: None,
            extra_flags: vec![],
        }
    }

    #[test]
    fn build_command_includes_print_flag() {
        let cmd = adapter().build_command(&default_config());
        let args = cmd_args(&cmd);
        assert!(args.contains(&"--print".to_string()), "--print must be in args (required for stream-json), got {:?}", args);
    }

    #[test]
    fn build_command_includes_output_format_stream_json() {
        let cmd = adapter().build_command(&default_config());
        let args = cmd_args(&cmd);
        let pos = args.iter().position(|a| a == "--output-format")
            .expect("--output-format flag must be present");
        assert_eq!(args[pos + 1], "stream-json", "next arg after --output-format must be stream-json");
    }

    #[test]
    fn build_command_includes_verbose_flag() {
        let cmd = adapter().build_command(&default_config());
        let args = cmd_args(&cmd);
        assert!(args.contains(&"--verbose".to_string()), "--verbose must be in args, got {:?}", args);
    }

    #[test]
    fn build_command_output_format_and_verbose_always_present() {
        // Regardless of what else is in config
        let config = AdapterConfig {
            working_dir: std::path::PathBuf::from("/tmp"),
            model: Some("claude-opus-4-5".to_string()),
            resume_session_id: Some("sess-xyz".to_string()),
            extra_flags: vec!["--dangerously-skip-permissions".to_string()],
        };
        let cmd = adapter().build_command(&config);
        let args = cmd_args(&cmd);
        assert!(args.contains(&"--print".to_string()), "--print must always be present");
        assert!(args.contains(&"--verbose".to_string()));
        let pos = args.iter().position(|a| a == "--output-format").expect("--output-format missing");
        assert_eq!(args[pos + 1], "stream-json");
    }

    // ── build_command: optional flags ─────────────────────────────────────────

    #[test]
    fn build_command_no_resume_when_not_configured() {
        let cmd = adapter().build_command(&default_config());
        let args = cmd_args(&cmd);
        assert!(!args.contains(&"--resume".to_string()), "--resume should not be present without config");
    }

    #[test]
    fn build_command_includes_resume_when_configured() {
        let config = AdapterConfig {
            resume_session_id: Some("sess-42".to_string()),
            ..default_config()
        };
        let cmd = adapter().build_command(&config);
        let args = cmd_args(&cmd);
        let pos = args.iter().position(|a| a == "--resume")
            .expect("--resume must be present when resume_session_id is set");
        assert_eq!(args[pos + 1], "sess-42");
    }

    #[test]
    fn build_command_no_model_when_not_configured() {
        let cmd = adapter().build_command(&default_config());
        let args = cmd_args(&cmd);
        assert!(!args.contains(&"--model".to_string()), "--model should not be present without config");
    }

    #[test]
    fn build_command_includes_model_when_configured() {
        let config = AdapterConfig {
            model: Some("claude-3-5-sonnet-20241022".to_string()),
            ..default_config()
        };
        let cmd = adapter().build_command(&config);
        let args = cmd_args(&cmd);
        let pos = args.iter().position(|a| a == "--model")
            .expect("--model must be present when model is set");
        assert_eq!(args[pos + 1], "claude-3-5-sonnet-20241022");
    }

    #[test]
    fn build_command_includes_extra_flags() {
        let config = AdapterConfig {
            extra_flags: vec!["--dangerously-skip-permissions".to_string(), "--headless".to_string()],
            ..default_config()
        };
        let cmd = adapter().build_command(&config);
        let args = cmd_args(&cmd);
        assert!(args.contains(&"--dangerously-skip-permissions".to_string()));
        assert!(args.contains(&"--headless".to_string()));
    }

    #[test]
    fn build_command_sets_working_dir() {
        let config = AdapterConfig {
            working_dir: std::path::PathBuf::from("/var/my-project"),
            ..default_config()
        };
        let cmd = adapter().build_command(&config);
        // Verify cwd is set on the underlying std::process::Command
        let cwd = cmd.as_std().get_current_dir();
        assert_eq!(cwd, Some(std::path::Path::new("/var/my-project")));
    }

    // ── detect: binary lookup ─────────────────────────────────────────────────

    #[test]
    fn detect_returns_path_or_none() {
        // We can't mock `which` in a unit test without a shim, so we just verify
        // the return type contract: if something is returned, it's an absolute path.
        let result = adapter().detect();
        if let Some(path) = result {
            assert!(path.is_absolute(), "detect() must return an absolute path, got {:?}", path);
        }
        // None is also valid on machines without claude installed.
    }

    #[test]
    fn detect_fallback_candidates_are_absolute() {
        // Verify the hardcoded fallback paths are absolute (regression guard).
        let fallbacks: Vec<PathBuf> = vec![
            PathBuf::from("/usr/local/bin/claude"),
            PathBuf::from("/usr/bin/claude"),
        ];
        for f in &fallbacks {
            assert!(f.is_absolute(), "fallback path {:?} must be absolute", f);
        }
        // home_dir fallback
        if let Some(home) = dirs::home_dir() {
            let home_fallback = home.join(".local/bin/claude");
            assert!(home_fallback.is_absolute(), "home fallback must be absolute");
        }
    }

    #[test]
    fn build_command_binary_falls_back_to_string_claude_when_not_found() {
        // When detect() returns None (no claude on PATH, no file at fallback paths),
        // build_command should still succeed and use "claude" as the binary name.
        // We can test this indirectly: build_command must not panic.
        // (The actual binary string is not directly readable from tokio::Command,
        // but the command is created successfully.)
        let cmd = adapter().build_command(&default_config());
        // If we got here without panic, fallback worked.
        let args = cmd_args(&cmd);
        // At minimum the required flags must still be present
        assert!(args.contains(&"--verbose".to_string()));
    }

    // ── capabilities ──────────────────────────────────────────────────────────

    #[test]
    fn capabilities_structured_and_tool_events() {
        let caps = adapter().capabilities();
        assert!(caps.structured_output);
        assert!(caps.tool_events);
        assert!(caps.approval_intercept);
    }

    #[test]
    fn capabilities_session_resumable() {
        // Claude supports --resume; session_resumable must be true.
        let caps = adapter().capabilities();
        assert!(caps.session_resumable, "Claude adapter must advertise session_resumable=true");
    }

    // ── name ──────────────────────────────────────────────────────────────────

    #[test]
    fn adapter_name_is_claude() {
        assert_eq!(adapter().name(), "claude");
    }
}
