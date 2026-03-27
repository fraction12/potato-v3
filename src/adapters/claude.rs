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
            session_resumable: false,
            approval_intercept: true,
            tool_events: true,
        }
    }

    /// Build the Claude CLI command.
    ///
    /// Produces: `claude --output-format stream-json --verbose [--resume id] [--model m] [flags…]`
    fn build_command(&self, config: &AdapterConfig) -> Command {
        let binary = self.detect().unwrap_or_else(|| PathBuf::from("claude"));
        let mut cmd = Command::new(binary);

        cmd.current_dir(&config.working_dir);
        cmd.args(["--output-format", "stream-json", "--verbose"]);

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
            // ── Session init ─────────────────────────────────────────────────
            "system" if value["subtype"].as_str() == Some("init") => {
                if let Some(session_id) = value["session_id"].as_str() {
                    vec![AgentEvent::SessionBound { agent_session_id: session_id.to_string() }]
                } else {
                    vec![AgentEvent::Raw { payload: trimmed.to_string() }]
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
                                let id = item["id"].as_str().unwrap_or("").to_string();
                                let name = item["name"].as_str().unwrap_or("").to_string();
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

            // ── Unknown / passthrough ─────────────────────────────────────────
            _ => {
                warn!("claude adapter: unrecognised line type {:?}", msg_type);
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

    fn adapter() -> ClaudeAdapter { ClaudeAdapter }

    #[test]
    fn parse_empty_line() {
        assert!(adapter().parse_line("").is_empty());
        assert!(adapter().parse_line("   ").is_empty());
    }

    #[test]
    fn parse_system_init() {
        let line = r#"{"type":"system","subtype":"init","session_id":"s-abc","tools":[]}"#;
        let events = adapter().parse_line(line);
        assert_eq!(events.len(), 1);
        assert!(matches!(&events[0], AgentEvent::SessionBound { agent_session_id } if agent_session_id == "s-abc"));
    }

    #[test]
    fn parse_text_delta() {
        let line = r#"{"type":"assistant","message":{"content":[{"type":"text","text":"Hello!"}]}}"#;
        let events = adapter().parse_line(line);
        assert_eq!(events.len(), 1);
        assert!(matches!(&events[0], AgentEvent::TextDelta { text } if text == "Hello!"));
    }

    #[test]
    fn parse_tool_use() {
        let line = r#"{"type":"assistant","message":{"content":[{"type":"tool_use","id":"t1","name":"read_file","input":{"path":"/tmp/x"}}]}}"#;
        let events = adapter().parse_line(line);
        assert_eq!(events.len(), 1);
        assert!(matches!(&events[0], AgentEvent::ToolStart { id, name, .. } if id == "t1" && name == "read_file"));
    }

    #[test]
    fn parse_result_success() {
        let line = r#"{"type":"result","subtype":"success","result":"All done","usage":{"input_tokens":100,"output_tokens":50},"cost_usd":0.001}"#;
        let events = adapter().parse_line(line);
        // Should have TextDone and TurnDone
        assert!(events.iter().any(|e| matches!(e, AgentEvent::TextDone { .. })));
        assert!(events.iter().any(|e| matches!(e, AgentEvent::TurnDone { usage: Some(_) })));
    }

    #[test]
    fn parse_result_error_subtype() {
        let line = r#"{"type":"result","subtype":"error","error":"Something failed","usage":null}"#;
        let events = adapter().parse_line(line);
        assert!(events.iter().any(|e| matches!(e, AgentEvent::Error { .. })));
    }

    #[test]
    fn parse_user_message_returns_empty() {
        let line = r#"{"type":"user","message":{"content":[{"type":"text","text":"hi"}]}}"#;
        let events = adapter().parse_line(line);
        assert!(events.is_empty());
    }

    #[test]
    fn parse_unknown_type_is_raw() {
        let line = r#"{"type":"debug","payload":"something"}"#;
        let events = adapter().parse_line(line);
        assert_eq!(events.len(), 1);
        assert!(matches!(&events[0], AgentEvent::Raw { .. }));
    }

    #[test]
    fn format_user_input_appends_newline() {
        assert_eq!(adapter().format_user_input("hello world"), "hello world\n");
    }

    #[test]
    fn format_approval_yes() {
        assert_eq!(adapter().format_approval(true), Some("y\n".to_string()));
    }

    #[test]
    fn format_approval_no() {
        assert_eq!(adapter().format_approval(false), Some("n\n".to_string()));
    }
}
