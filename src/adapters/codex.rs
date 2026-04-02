//! Codex adapter — spawns `codex` (interactive PTY mode) and parses
//! the JSONL output from `codex exec --json` into canonical [`AgentEvent`]s.
//!
//! Codex is interactive by default (no `--print` needed).  For structured
//! output in non-interactive mode use `codex exec --json`; in PTY mode the
//! events are emitted to the terminal stream but the same JSONL schema applies.

use std::path::PathBuf;

use tokio::process::Command;

use super::{AdapterCapabilities, AdapterConfig, AgentAdapter};
use crate::events::{AgentEvent, UsageInfo};

// ── CodexAdapter ──────────────────────────────────────────────────────────────

/// Adapter for the OpenAI Codex CLI (`codex`).
pub struct CodexAdapter;

impl AgentAdapter for CodexAdapter {
    fn name(&self) -> &str {
        "codex"
    }

    /// Locate the `codex` binary via PATH or common install locations.
    fn detect(&self) -> Option<PathBuf> {
        // Try PATH first.
        if let Ok(p) = which::which("codex") {
            return Some(p);
        }
        // Common macOS/Linux install paths.
        let candidates: Vec<PathBuf> = vec![
            PathBuf::from("/opt/homebrew/bin/codex"),
            PathBuf::from("/usr/local/bin/codex"),
            PathBuf::from("/usr/bin/codex"),
        ];
        candidates.iter().find(|c| c.exists()).cloned()
    }

    fn capabilities(&self) -> AdapterCapabilities {
        AdapterCapabilities {
            // Codex emits JSONL with `codex exec --json`; PTY mode also streams events.
            structured_output: true,
            // Codex supports `codex resume <session_id>`.
            session_resumable: true,
            // Codex manages its own approval flow internally.
            approval_intercept: false,
            // item.started / item.completed events.
            tool_events: true,
        }
    }

    /// Build the Codex CLI command.
    ///
    /// Interactive mode (PTY): `codex [resume <id>] [-m <model>] [flags…]`
    ///
    /// Unlike Claude, Codex is interactive by default and does not need `--print`.
    /// The working dir is set via `current_dir`.
    fn build_command(&self, config: &AdapterConfig) -> Command {
        let binary = self.detect().unwrap_or_else(|| PathBuf::from("codex"));
        let mut cmd = Command::new(binary);

        cmd.current_dir(&config.working_dir);

        if let Some(ref session_id) = config.resume_session_id {
            cmd.arg("resume").arg(session_id);
        }

        if let Some(ref model) = config.model {
            cmd.args(["-m", model]);
        }

        for flag in &config.extra_flags {
            cmd.arg(flag);
        }

        cmd
    }

    /// Parse a single JSONL line from Codex's event stream.
    ///
    /// Codex event schema:
    /// - `{"type":"thread.started","thread_id":"<uuid>"}` → [`AgentEvent::SessionBound`]
    /// - `{"type":"turn.started"}` → ignored (no useful data)
    /// - `{"type":"item.started","item":{"type":"command_execution","id":"…","command":"…"}}` → [`AgentEvent::ToolStart`]
    /// - `{"type":"item.completed","item":{"type":"command_execution","id":"…","aggregated_output":"…","exit_code":0}}` → [`AgentEvent::ToolDone`]
    /// - `{"type":"item.completed","item":{"type":"agent_message","text":"…"}}` → [`AgentEvent::TextDone`]
    /// - `{"type":"turn.completed","usage":{…}}` → [`AgentEvent::TurnDone`]
    /// - Anything else → [`AgentEvent::Raw`]
    fn parse_line(&self, line: &str) -> Vec<AgentEvent> {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            return vec![];
        }

        let value: serde_json::Value = match serde_json::from_str(trimmed) {
            Ok(v) => v,
            Err(_) => {
                return vec![AgentEvent::Raw {
                    payload: trimmed.to_string(),
                }];
            }
        };

        let event_type = value["type"].as_str().unwrap_or("");

        match event_type {
            // ── Session binding ───────────────────────────────────────────────
            "thread.started" => {
                if let Some(thread_id) = value["thread_id"].as_str() {
                    vec![AgentEvent::SessionBound {
                        agent_session_id: thread_id.to_string(),
                    }]
                } else {
                    vec![AgentEvent::Raw {
                        payload: trimmed.to_string(),
                    }]
                }
            }

            // ── Turn lifecycle ────────────────────────────────────────────────
            "turn.started" => {
                // No useful data — silently ignore.
                vec![]
            }

            "turn.completed" => {
                let usage = parse_codex_usage(&value);
                vec![AgentEvent::TurnDone { usage }]
            }

            // ── Item events ───────────────────────────────────────────────────
            "item.started" => {
                let item = &value["item"];
                match item["type"].as_str() {
                    Some("command_execution") => {
                        let id = item["id"].as_str().unwrap_or("").to_string();
                        let command = item["command"].as_str().unwrap_or("").to_string();
                        let input = serde_json::json!({ "command": command });
                        vec![AgentEvent::ToolStart {
                            id,
                            name: "shell".to_string(),
                            input,
                        }]
                    }
                    _ => {
                        // Unknown item type — pass through as raw.
                        vec![AgentEvent::Raw {
                            payload: trimmed.to_string(),
                        }]
                    }
                }
            }

            "item.completed" => {
                let item = &value["item"];
                match item["type"].as_str() {
                    Some("command_execution") => {
                        let id = item["id"].as_str().unwrap_or("").to_string();
                        let output = item["aggregated_output"].as_str().unwrap_or("").to_string();
                        let exit_code = item["exit_code"].as_i64();
                        let success = exit_code.map(|c| c == 0).unwrap_or(true);
                        vec![AgentEvent::ToolDone {
                            id,
                            output,
                            duration_ms: 0,
                            success,
                        }]
                    }
                    Some("agent_message") => {
                        let text = item["text"].as_str().unwrap_or("").to_string();
                        if text.is_empty() {
                            vec![AgentEvent::Raw {
                                payload: trimmed.to_string(),
                            }]
                        } else {
                            vec![AgentEvent::TextDone {
                                full_text: text,
                                turn_id: None,
                            }]
                        }
                    }
                    _ => {
                        vec![AgentEvent::Raw {
                            payload: trimmed.to_string(),
                        }]
                    }
                }
            }

            // ── Unknown / passthrough ─────────────────────────────────────────
            _ => {
                vec![AgentEvent::Raw {
                    payload: trimmed.to_string(),
                }]
            }
        }
    }

    /// Format user input for Codex's stdin: raw text followed by a newline.
    fn format_user_input(&self, text: &str) -> String {
        format!("{text}\n")
    }

    /// Codex manages its own approval flow — no intercept support.
    fn format_approval(&self, _approved: bool) -> Option<String> {
        None
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Extract usage information from a `turn.completed` event, if present.
fn parse_codex_usage(value: &serde_json::Value) -> Option<UsageInfo> {
    let usage = &value["usage"];
    if usage.is_null() {
        return None;
    }
    let input_tokens = usage["input_tokens"].as_u64().unwrap_or(0);
    let cached_input_tokens = usage["cached_input_tokens"].as_u64().unwrap_or(0);
    let output_tokens = usage["output_tokens"].as_u64().unwrap_or(0);

    // Use net input (non-cached) for billing consistency; total is input + cached.
    let total_input = input_tokens.saturating_add(cached_input_tokens);

    Some(UsageInfo {
        input_tokens: total_input,
        output_tokens,
        cost_usd: None, // Codex does not report cost in turn.completed
    })
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;

    fn adapter() -> CodexAdapter {
        CodexAdapter
    }

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

    // ── name / capabilities ───────────────────────────────────────────────────

    #[test]
    fn adapter_name_is_codex() {
        assert_eq!(adapter().name(), "codex");
    }

    #[test]
    fn capabilities_structured_output() {
        assert!(adapter().capabilities().structured_output);
    }

    #[test]
    fn capabilities_session_resumable() {
        assert!(adapter().capabilities().session_resumable);
    }

    #[test]
    fn capabilities_no_approval_intercept() {
        assert!(!adapter().capabilities().approval_intercept);
    }

    #[test]
    fn capabilities_tool_events() {
        assert!(adapter().capabilities().tool_events);
    }

    // ── detect ────────────────────────────────────────────────────────────────

    #[test]
    fn detect_returns_absolute_path_or_none() {
        if let Some(path) = adapter().detect() {
            assert!(
                path.is_absolute(),
                "detect() must return an absolute path, got {:?}",
                path
            );
        }
        // None is valid on machines without codex installed.
    }

    #[test]
    fn detect_fallback_candidates_are_absolute() {
        let fallbacks: Vec<PathBuf> = vec![
            PathBuf::from("/opt/homebrew/bin/codex"),
            PathBuf::from("/usr/local/bin/codex"),
            PathBuf::from("/usr/bin/codex"),
        ];
        for f in &fallbacks {
            assert!(f.is_absolute(), "fallback path {:?} must be absolute", f);
        }
    }

    // ── build_command ─────────────────────────────────────────────────────────

    #[test]
    fn build_command_no_print_flag() {
        // Unlike Claude, Codex does NOT need --print.
        let cmd = adapter().build_command(&default_config());
        let args = cmd_args(&cmd);
        assert!(
            !args.contains(&"--print".to_string()),
            "--print must NOT be in codex args, got {:?}",
            args
        );
    }

    #[test]
    fn build_command_no_output_format_flag() {
        let cmd = adapter().build_command(&default_config());
        let args = cmd_args(&cmd);
        assert!(
            !args.contains(&"--output-format".to_string()),
            "--output-format must NOT be in codex args"
        );
    }

    #[test]
    fn build_command_no_args_by_default() {
        let cmd = adapter().build_command(&default_config());
        let args = cmd_args(&cmd);
        // With default config (no resume, no model, no extra flags), args should be empty.
        assert!(
            args.is_empty(),
            "default config should produce no args, got {:?}",
            args
        );
    }

    #[test]
    fn build_command_resume_flag() {
        let config = AdapterConfig {
            resume_session_id: Some("abc-123".to_string()),
            ..default_config()
        };
        let cmd = adapter().build_command(&config);
        let args = cmd_args(&cmd);
        // Should produce: resume abc-123
        assert_eq!(args[0], "resume", "first arg must be 'resume'");
        assert_eq!(args[1], "abc-123", "second arg must be the session id");
    }

    #[test]
    fn build_command_model_flag() {
        let config = AdapterConfig {
            model: Some("gpt-4o".to_string()),
            ..default_config()
        };
        let cmd = adapter().build_command(&config);
        let args = cmd_args(&cmd);
        let pos = args
            .iter()
            .position(|a| a == "-m")
            .expect("-m must be present when model is set");
        assert_eq!(args[pos + 1], "gpt-4o");
    }

    #[test]
    fn build_command_extra_flags() {
        let config = AdapterConfig {
            extra_flags: vec![
                "--full-auto".to_string(),
                "--dangerously-bypass-approvals-and-sandbox".to_string(),
            ],
            ..default_config()
        };
        let cmd = adapter().build_command(&config);
        let args = cmd_args(&cmd);
        assert!(args.contains(&"--full-auto".to_string()));
        assert!(args.contains(&"--dangerously-bypass-approvals-and-sandbox".to_string()));
    }

    #[test]
    fn build_command_sets_working_dir() {
        let config = AdapterConfig {
            working_dir: std::path::PathBuf::from("/var/myproject"),
            ..default_config()
        };
        let cmd = adapter().build_command(&config);
        assert_eq!(
            cmd.as_std().get_current_dir(),
            Some(std::path::Path::new("/var/myproject"))
        );
    }

    #[test]
    fn build_command_resume_and_model_order() {
        // resume comes before model flag.
        let config = AdapterConfig {
            resume_session_id: Some("sess-xyz".to_string()),
            model: Some("o4-mini".to_string()),
            ..default_config()
        };
        let cmd = adapter().build_command(&config);
        let args = cmd_args(&cmd);
        let resume_pos = args
            .iter()
            .position(|a| a == "resume")
            .expect("resume missing");
        let model_pos = args.iter().position(|a| a == "-m").expect("-m missing");
        assert!(resume_pos < model_pos, "resume must come before -m in args");
    }

    // ── parse_line: empty / whitespace ────────────────────────────────────────

    #[test]
    fn parse_empty_line_returns_empty() {
        assert!(adapter().parse_line("").is_empty());
        assert!(adapter().parse_line("   ").is_empty());
        assert!(adapter().parse_line("\t\n").is_empty());
    }

    // ── parse_line: invalid JSON → Raw ────────────────────────────────────────

    #[test]
    fn parse_invalid_json_returns_raw() {
        let events = adapter().parse_line("not json");
        assert_eq!(events.len(), 1);
        assert!(matches!(&events[0], AgentEvent::Raw { payload } if payload == "not json"));
    }

    // ── parse_line: thread.started → SessionBound ─────────────────────────────

    #[test]
    fn parse_thread_started_session_bound() {
        let line = r#"{"type":"thread.started","thread_id":"abc-uuid-123"}"#;
        let events = adapter().parse_line(line);
        assert_eq!(events.len(), 1);
        assert!(
            matches!(&events[0], AgentEvent::SessionBound { agent_session_id } if agent_session_id == "abc-uuid-123"),
            "expected SessionBound, got {:?}",
            events[0]
        );
    }

    #[test]
    fn parse_thread_started_missing_thread_id_returns_raw() {
        let line = r#"{"type":"thread.started"}"#;
        let events = adapter().parse_line(line);
        assert_eq!(events.len(), 1);
        assert!(matches!(&events[0], AgentEvent::Raw { .. }));
    }

    // ── parse_line: turn.started → empty ─────────────────────────────────────

    #[test]
    fn parse_turn_started_returns_empty() {
        let line = r#"{"type":"turn.started"}"#;
        let events = adapter().parse_line(line);
        assert!(events.is_empty(), "turn.started should be silently ignored");
    }

    // ── parse_line: turn.completed → TurnDone ────────────────────────────────

    #[test]
    fn parse_turn_completed_turn_done() {
        let line = r#"{"type":"turn.completed","usage":{"input_tokens":63773,"cached_input_tokens":44544,"output_tokens":650}}"#;
        let events = adapter().parse_line(line);
        assert_eq!(events.len(), 1);
        assert!(
            matches!(&events[0], AgentEvent::TurnDone { .. }),
            "expected TurnDone, got {:?}",
            events[0]
        );
    }

    #[test]
    fn parse_turn_completed_usage_values() {
        let line = r#"{"type":"turn.completed","usage":{"input_tokens":100,"cached_input_tokens":50,"output_tokens":200}}"#;
        let events = adapter().parse_line(line);
        if let AgentEvent::TurnDone { usage: Some(u) } = &events[0] {
            // input = 100 + 50 (cached) = 150
            assert_eq!(u.input_tokens, 150);
            assert_eq!(u.output_tokens, 200);
            assert!(u.cost_usd.is_none());
        } else {
            panic!("expected TurnDone with Some(usage), got {:?}", events[0]);
        }
    }

    #[test]
    fn parse_turn_completed_no_usage_gives_turn_done_none() {
        let line = r#"{"type":"turn.completed"}"#;
        let events = adapter().parse_line(line);
        assert!(matches!(&events[0], AgentEvent::TurnDone { usage: None }));
    }

    // ── parse_line: item.started command_execution → ToolStart ───────────────

    #[test]
    fn parse_item_started_command_execution_tool_start() {
        let line = r#"{"type":"item.started","item":{"id":"item_0","type":"command_execution","command":"ls -la","aggregated_output":"","exit_code":null,"status":"in_progress"}}"#;
        let events = adapter().parse_line(line);
        assert_eq!(events.len(), 1);
        assert!(
            matches!(&events[0], AgentEvent::ToolStart { id, name, .. } if id == "item_0" && name == "shell"),
            "expected ToolStart(item_0, shell), got {:?}",
            events[0]
        );
    }

    #[test]
    fn parse_item_started_command_preserved_in_input() {
        let line = r#"{"type":"item.started","item":{"id":"item_1","type":"command_execution","command":"echo hello","aggregated_output":"","exit_code":null,"status":"in_progress"}}"#;
        let events = adapter().parse_line(line);
        if let AgentEvent::ToolStart { id, input, .. } = &events[0] {
            assert_eq!(id, "item_1");
            assert_eq!(input["command"].as_str(), Some("echo hello"));
        } else {
            panic!("expected ToolStart, got {:?}", events[0]);
        }
    }

    #[test]
    fn parse_item_started_unknown_type_returns_raw() {
        let line = r#"{"type":"item.started","item":{"id":"item_0","type":"unknown_thing"}}"#;
        let events = adapter().parse_line(line);
        assert!(matches!(&events[0], AgentEvent::Raw { .. }));
    }

    // ── parse_line: item.completed command_execution → ToolDone ──────────────

    #[test]
    fn parse_item_completed_command_execution_tool_done() {
        let line = r#"{"type":"item.completed","item":{"id":"item_0","type":"command_execution","command":"ls","aggregated_output":"file1\nfile2\n","exit_code":0,"status":"completed"}}"#;
        let events = adapter().parse_line(line);
        assert_eq!(events.len(), 1);
        assert!(
            matches!(&events[0], AgentEvent::ToolDone { id, success, .. } if id == "item_0" && *success),
            "expected ToolDone(item_0, success=true), got {:?}",
            events[0]
        );
    }

    #[test]
    fn parse_item_completed_command_output_preserved() {
        let line = r#"{"type":"item.completed","item":{"id":"item_2","type":"command_execution","command":"cat","aggregated_output":"hello world","exit_code":0,"status":"completed"}}"#;
        let events = adapter().parse_line(line);
        if let AgentEvent::ToolDone {
            id,
            output,
            success,
            ..
        } = &events[0]
        {
            assert_eq!(id, "item_2");
            assert_eq!(output, "hello world");
            assert!(*success);
        } else {
            panic!("expected ToolDone, got {:?}", events[0]);
        }
    }

    #[test]
    fn parse_item_completed_nonzero_exit_code_is_failure() {
        let line = r#"{"type":"item.completed","item":{"id":"item_3","type":"command_execution","command":"false","aggregated_output":"","exit_code":1,"status":"completed"}}"#;
        let events = adapter().parse_line(line);
        assert!(
            matches!(&events[0], AgentEvent::ToolDone { success, .. } if !*success),
            "exit_code 1 should produce success=false, got {:?}",
            events[0]
        );
    }

    // ── parse_line: item.completed agent_message → TextDone ──────────────────

    #[test]
    fn parse_item_completed_agent_message_text_done() {
        let line = r#"{"type":"item.completed","item":{"id":"item_5","type":"agent_message","text":"I've finished the task."}}"#;
        let events = adapter().parse_line(line);
        assert_eq!(events.len(), 1);
        assert!(
            matches!(&events[0], AgentEvent::TextDone { full_text, .. } if full_text == "I've finished the task."),
            "expected TextDone, got {:?}",
            events[0]
        );
    }

    #[test]
    fn parse_item_completed_agent_message_empty_returns_raw() {
        let line =
            r#"{"type":"item.completed","item":{"id":"item_6","type":"agent_message","text":""}}"#;
        let events = adapter().parse_line(line);
        // Empty text → Raw fallback
        assert!(matches!(&events[0], AgentEvent::Raw { .. }));
    }

    #[test]
    fn parse_item_completed_unknown_item_type_returns_raw() {
        let line =
            r#"{"type":"item.completed","item":{"id":"item_7","type":"something_new","data":"x"}}"#;
        let events = adapter().parse_line(line);
        assert!(matches!(&events[0], AgentEvent::Raw { .. }));
    }

    // ── parse_line: unknown type → Raw ────────────────────────────────────────

    #[test]
    fn parse_unknown_event_type_is_raw() {
        let line = r#"{"type":"debug","payload":"something"}"#;
        let events = adapter().parse_line(line);
        assert!(matches!(&events[0], AgentEvent::Raw { .. }));
    }

    // ── format_user_input ─────────────────────────────────────────────────────

    #[test]
    fn format_user_input_appends_newline() {
        assert_eq!(adapter().format_user_input("hello"), "hello\n");
    }

    #[test]
    fn format_user_input_empty_string() {
        assert_eq!(adapter().format_user_input(""), "\n");
    }

    // ── format_approval ───────────────────────────────────────────────────────

    #[test]
    fn format_approval_returns_none() {
        assert!(adapter().format_approval(true).is_none());
        assert!(adapter().format_approval(false).is_none());
    }
}
