//! Historical session discovery — scans `~/.claude/projects/` and upserts
//! all known JSONL session files into the SQLite store.
//!
//! Runs once at startup. Not latency-sensitive.

use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use anyhow::Result;
use serde_json::Value;

use crate::claude_log::{ClaudeSessionLogTracker, claude_projects_dir};
use crate::session::store::{SessionEvent, SessionStore, unix_now};

// ── Discovery ─────────────────────────────────────────────────────────────────

/// Scan `~/.claude/projects/` and upsert all found sessions into `store`.
///
/// For each `<project-dir>/<session-id>.jsonl` file:
/// - Parse it with [`parse_jsonl_file`]
/// - Upsert the session row (idempotent via ON CONFLICT)
///
/// Errors from individual files are logged and skipped; the scan continues.
pub fn discover_historical_sessions(home: &Path, store: &SessionStore) {
    let projects_dir = claude_projects_dir(home);
    if !projects_dir.exists() {
        return;
    }

    let project_entries = match fs::read_dir(&projects_dir) {
        Ok(e) => e,
        Err(err) => {
            tracing::warn!(
                "discover_historical_sessions: cannot read {:?}: {}",
                projects_dir,
                err
            );
            return;
        }
    };

    for project_entry in project_entries {
        let project_entry = match project_entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        let project_path = project_entry.path();
        if !project_path.is_dir() {
            continue;
        }

        let project_dir = project_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();

        let jsonl_entries = match fs::read_dir(&project_path) {
            Ok(e) => e,
            Err(_) => continue,
        };

        for jsonl_entry in jsonl_entries {
            let jsonl_entry = match jsonl_entry {
                Ok(e) => e,
                Err(_) => continue,
            };
            let jsonl_path = jsonl_entry.path();
            if jsonl_path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                continue;
            }

            let session_id = match jsonl_path.file_stem().and_then(|s| s.to_str()) {
                Some(s) => s.to_string(),
                None => continue,
            };

            if let Err(e) = ingest_jsonl_file(store, &jsonl_path, &session_id, &project_dir) {
                tracing::debug!("discover: skipped {:?}: {}", jsonl_path, e);
            }
        }
    }
}

// ── Per-file ingestion ────────────────────────────────────────────────────────

/// Parse a single JSONL file and upsert it into the store.
fn ingest_jsonl_file(
    store: &SessionStore,
    path: &Path,
    session_id: &str,
    project_dir: &str,
) -> Result<()> {
    let parsed = parse_jsonl_file(path, session_id, project_dir)?;

    store.upsert_session(
        &parsed.session_id,
        &parsed.project_dir,
        &parsed.agent,
        parsed.model.as_deref(),
        &parsed.title,
        None, // cwd not derivable from JSONL alone
        parsed.total_input_tokens,
        parsed.total_output_tokens,
        parsed.turn_count,
        parsed.created_at,
        parsed.updated_at,
    )?;

    Ok(())
}

// ── JSONL parsing ─────────────────────────────────────────────────────────────

/// Parsed summary extracted from a JSONL session file.
#[derive(Debug)]
pub struct ParsedSession {
    pub session_id: String,
    pub project_dir: String,
    pub agent: String,
    pub model: Option<String>,
    pub title: String,
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
    pub turn_count: u64,
    pub created_at: i64,
    pub updated_at: i64,
}

/// Parse a JSONL file using [`ClaudeSessionLogTracker::process_line`] for line
/// handling and extract the fields needed for the sessions table.
///
/// Also extracts the first user prompt text as the session title.
pub fn parse_jsonl_file(path: &Path, session_id: &str, project_dir: &str) -> Result<ParsedSession> {
    let file = fs::File::open(path)?;
    let reader = BufReader::new(file);

    let mut tracker = ClaudeSessionLogTracker::default();
    let mut first_user_prompt: Option<String> = None;
    let mut first_ts: Option<i64> = None;
    let mut last_ts: Option<i64> = None;

    for line_result in reader.lines() {
        let line = match line_result {
            Ok(l) => l,
            Err(_) => break,
        };
        if line.trim().is_empty() {
            continue;
        }

        // Extract timestamp from the raw JSON.
        if let Ok(v) = serde_json::from_str::<Value>(&line) {
            if let Some(ts_str) = v.get("timestamp").and_then(Value::as_str) {
                if let Ok(ts) = chrono::DateTime::parse_from_rfc3339(ts_str) {
                    let secs = ts.timestamp();
                    if first_ts.is_none() {
                        first_ts = Some(secs);
                    }
                    last_ts = Some(secs);
                }
            }

            // Extract first user prompt for the title.
            if first_user_prompt.is_none() {
                if let Some(msg) = v.get("message") {
                    if msg.get("role").and_then(Value::as_str) == Some("user") {
                        if let Some(content) = msg.get("content") {
                            // Content may be a string or an array.
                            let prompt = extract_user_text(content);
                            if !prompt.is_empty() {
                                first_user_prompt = Some(truncate_str(&prompt, 80));
                            }
                        }
                    }
                }
            }
        }

        // Feed through the tracker for token/turn accounting.
        tracker.process_line(&line);
    }

    let now = unix_now();
    let snap = tracker.snapshot();

    Ok(ParsedSession {
        session_id: session_id.to_string(),
        project_dir: project_dir.to_string(),
        agent: "claude".to_string(),
        model: snap.model,
        title: first_user_prompt.unwrap_or_default(),
        total_input_tokens: snap.usage.input_tokens,
        total_output_tokens: snap.usage.output_tokens,
        turn_count: snap.turns,
        created_at: first_ts.unwrap_or(now),
        updated_at: last_ts.unwrap_or(now),
    })
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Extract readable text from a user message `content` value (string or array).
fn extract_user_text(content: &Value) -> String {
    match content {
        Value::String(s) => s.clone(),
        Value::Array(items) => {
            for item in items {
                if item.get("type").and_then(Value::as_str) == Some("text") {
                    if let Some(text) = item.get("text").and_then(Value::as_str) {
                        return text.to_string();
                    }
                }
            }
            String::new()
        }
        _ => String::new(),
    }
}

use crate::util::truncate_str;

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_temp_jsonl(lines: &[&str]) -> PathBuf {
        let tmp = std::env::temp_dir().join(format!("potato-disc-{}.jsonl", std::process::id()));
        let mut f = fs::File::create(&tmp).unwrap();
        for line in lines {
            writeln!(f, "{}", line).unwrap();
        }
        tmp
    }

    #[test]
    fn parse_extracts_title_and_tokens() {
        let line_user = r#"{"timestamp":"2024-03-25T10:00:00Z","type":"user","message":{"role":"user","content":[{"type":"text","text":"Refactor the auth module"}]}}"#;
        let line_asst = r#"{"timestamp":"2024-03-25T10:00:05Z","type":"assistant","message":{"role":"assistant","model":"claude-sonnet-4-6","stop_reason":"end_turn","usage":{"input_tokens":50,"output_tokens":80},"content":[]}}"#;

        let tmp = write_temp_jsonl(&[line_user, line_asst]);
        let parsed = parse_jsonl_file(&tmp, "sess-abc", "proj-foo").unwrap();
        let _ = fs::remove_file(&tmp);

        assert_eq!(parsed.session_id, "sess-abc");
        assert_eq!(parsed.project_dir, "proj-foo");
        assert_eq!(parsed.title, "Refactor the auth module");
        assert_eq!(parsed.total_input_tokens, 50);
        assert_eq!(parsed.total_output_tokens, 80);
        assert_eq!(parsed.turn_count, 1);
        assert_eq!(parsed.model.as_deref(), Some("claude-sonnet-4-6"));
        assert_eq!(parsed.created_at, 1_711_360_800);
    }

    #[test]
    fn truncate_clips_long_strings() {
        let s = "a".repeat(100);
        let t = truncate_str(&s, 80);
        // 79 chars + ellipsis = 80 chars total
        assert_eq!(t.chars().count(), 80);
    }

    #[test]
    fn truncate_leaves_short_strings() {
        let s = "hello";
        assert_eq!(truncate_str(s, 80), "hello");
    }

    #[test]
    fn extract_user_text_from_string() {
        let v = Value::String("plain text".into());
        assert_eq!(extract_user_text(&v), "plain text");
    }

    #[test]
    fn extract_user_text_from_array() {
        let v: Value = serde_json::json!([{"type":"text","text":"array text"}]);
        assert_eq!(extract_user_text(&v), "array text");
    }
}
