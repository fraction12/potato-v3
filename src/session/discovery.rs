//! Historical session discovery at startup.
//!
//! Today this ingests Claude Code JSONL history from `~/.claude/projects/` and
//! Codex JSONL history from `~/.codex/sessions/`. The module surface is
//! intentionally provider-neutral so additional providers can be added without
//! rewriting startup call sites.
//!
//! Runs once at startup. Not latency-sensitive.

use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use anyhow::Result;
use serde_json::Value;

use crate::claude_log::ClaudeSessionLogTracker;
use crate::codex_log::{CodexSessionLogTracker, codex_sessions_dir};
use crate::session::store::{SessionStore, SessionUpsert, unix_now};
use crate::util::truncate_str;

/// Startup discovery configuration for a single provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HistoricalSessionDiscovery {
    Claude,
    Codex,
}

/// Discover historical sessions for all currently supported providers.
pub fn discover_historical_sessions(home: &Path, store: &SessionStore) {
    for provider in HistoricalSessionDiscovery::supported() {
        discover_historical_sessions_for(home, store, *provider);
    }
}

/// Discover historical sessions for one provider.
pub fn discover_historical_sessions_for(
    home: &Path,
    store: &SessionStore,
    provider: HistoricalSessionDiscovery,
) {
    match provider {
        HistoricalSessionDiscovery::Claude => discover_claude_historical_sessions(home, store),
        HistoricalSessionDiscovery::Codex => discover_codex_historical_sessions(home, store),
    }
}

impl HistoricalSessionDiscovery {
    const fn supported() -> &'static [Self] {
        &[Self::Claude, Self::Codex]
    }
}

/// Scan Claude Code's `~/.claude/projects/` history and upsert all found sessions into `store`.
///
/// For each `<project-dir>/<session-id>.jsonl` file:
/// - Parse it with [`parse_claude_jsonl_file`]
/// - Upsert the session row (idempotent via ON CONFLICT)
///
/// Errors from individual files are logged and skipped; the scan continues.
pub fn discover_claude_historical_sessions(home: &Path, store: &SessionStore) {
    let projects_dir = home.join(".claude").join("projects");
    if !projects_dir.exists() {
        return;
    }

    let project_entries = match fs::read_dir(&projects_dir) {
        Ok(e) => e,
        Err(err) => {
            tracing::warn!(
                "discover_claude_historical_sessions: cannot read {:?}: {}",
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

            if let Err(e) = ingest_claude_jsonl_file(store, &jsonl_path, &session_id, &project_dir)
            {
                tracing::debug!("discover_claude: skipped {:?}: {}", jsonl_path, e);
            }
        }
    }
}

/// Scan Codex's `~/.codex/sessions/` history and upsert all found sessions into `store`.
///
/// For each `YYYY/MM/DD/rollout-*.jsonl` file:
/// - Parse it with [`parse_codex_jsonl_file`]
/// - Upsert the session row (idempotent via ON CONFLICT)
///
/// Errors from individual files are logged and skipped; the scan continues.
pub fn discover_codex_historical_sessions(home: &Path, store: &SessionStore) {
    let sessions_dir = codex_sessions_dir(home);
    if !sessions_dir.exists() {
        return;
    }

    for jsonl_path in walk_codex_rollout_files(&sessions_dir) {
        if let Err(err) = ingest_codex_jsonl_file(store, &jsonl_path) {
            tracing::debug!("discover_codex: skipped {:?}: {}", jsonl_path, err);
        }
    }
}

/// Parse a single Claude JSONL file and upsert it into the store.
fn ingest_claude_jsonl_file(
    store: &SessionStore,
    path: &Path,
    session_id: &str,
    project_dir: &str,
) -> Result<()> {
    let parsed = parse_claude_jsonl_file(path, session_id, project_dir)?;

    store.upsert_session(&SessionUpsert {
        id: &parsed.session_id,
        project_dir: &parsed.project_dir,
        agent: &parsed.agent,
        model: parsed.model.as_deref(),
        title: &parsed.title,
        cwd: None,
        total_input_tokens: parsed.total_input_tokens,
        total_output_tokens: parsed.total_output_tokens,
        turn_count: parsed.turn_count,
        created_at: parsed.created_at,
        updated_at: parsed.updated_at,
    })?;

    Ok(())
}

fn ingest_codex_jsonl_file(store: &SessionStore, path: &Path) -> Result<()> {
    let parsed = parse_codex_jsonl_file(path)?;

    store.upsert_session(&SessionUpsert {
        id: &parsed.session_id,
        project_dir: &parsed.project_dir,
        agent: &parsed.agent,
        model: parsed.model.as_deref(),
        title: &parsed.title,
        cwd: None,
        total_input_tokens: parsed.total_input_tokens,
        total_output_tokens: parsed.total_output_tokens,
        turn_count: parsed.turn_count,
        created_at: parsed.created_at,
        updated_at: parsed.updated_at,
    })?;

    Ok(())
}

/// Parsed summary extracted from a discovered session log file.
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

/// Parse a Claude JSONL file using [`ClaudeSessionLogTracker::process_line`] for
/// line handling and extract the fields needed for the sessions table.
///
/// Also extracts the first user prompt text as the session title.
pub fn parse_claude_jsonl_file(
    path: &Path,
    session_id: &str,
    project_dir: &str,
) -> Result<ParsedSession> {
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

            if first_user_prompt.is_none() {
                if let Some(msg) = v.get("message") {
                    if msg.get("role").and_then(Value::as_str) == Some("user") {
                        if let Some(content) = msg.get("content") {
                            let prompt = extract_user_text(content);
                            if !prompt.is_empty() {
                                first_user_prompt = Some(truncate_str(&prompt, 80));
                            }
                        }
                    }
                }
            }
        }

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

/// Parse a Codex JSONL file using [`CodexSessionLogTracker::process_line`] for
/// line handling and extract the fields needed for the sessions table.
///
/// Title prefers the first user prompt and falls back to the first developer
/// prompt if there is no user-authored content.
pub fn parse_codex_jsonl_file(path: &Path) -> Result<ParsedSession> {
    let file = fs::File::open(path)?;
    let reader = BufReader::new(file);

    let mut tracker = CodexSessionLogTracker::default();
    let mut first_user_prompt: Option<String> = None;
    let mut first_developer_prompt: Option<String> = None;
    let mut first_ts: Option<i64> = None;
    let mut last_ts: Option<i64> = None;
    let mut session_id: Option<String> = None;
    let mut cwd: Option<String> = None;
    let mut model: Option<String> = None;

    for line_result in reader.lines() {
        let line = match line_result {
            Ok(l) => l,
            Err(_) => break,
        };
        if line.trim().is_empty() {
            continue;
        }

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

            match v.get("type").and_then(Value::as_str).unwrap_or_default() {
                "session_meta" => {
                    let payload = &v["payload"];
                    if session_id.is_none() {
                        session_id = payload
                            .get("id")
                            .and_then(Value::as_str)
                            .filter(|s| !s.is_empty())
                            .map(ToOwned::to_owned);
                    }
                    if cwd.is_none() {
                        cwd = payload
                            .get("cwd")
                            .and_then(Value::as_str)
                            .filter(|s| !s.is_empty())
                            .map(ToOwned::to_owned);
                    }
                    if model.is_none() {
                        model = payload
                            .get("model_slug")
                            .or_else(|| payload.get("model"))
                            .and_then(Value::as_str)
                            .filter(|s| !s.is_empty())
                            .map(ToOwned::to_owned);
                    }
                }
                "turn_context" => {
                    let payload = &v["payload"];
                    if cwd.is_none() {
                        cwd = payload
                            .get("cwd")
                            .and_then(Value::as_str)
                            .filter(|s| !s.is_empty())
                            .map(ToOwned::to_owned);
                    }
                    if model.is_none() {
                        model = payload
                            .get("model")
                            .and_then(Value::as_str)
                            .filter(|s| !s.is_empty())
                            .map(ToOwned::to_owned);
                    }
                }
                "response_item" => {
                    let payload = &v["payload"];
                    let role = payload
                        .get("role")
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    let text = extract_codex_response_text(payload.get("content"));
                    if !text.is_empty() {
                        let text = truncate_str(&text, 80);
                        if role == "user" && first_user_prompt.is_none() {
                            first_user_prompt = Some(text);
                        } else if role == "developer" && first_developer_prompt.is_none() {
                            first_developer_prompt = Some(text);
                        }
                    }
                }
                "event_msg" => {
                    let payload = &v["payload"];
                    if payload.get("type").and_then(Value::as_str) == Some("token_count") {
                        if model.is_none() {
                            model = payload
                                .get("model")
                                .or_else(|| payload.get("info").and_then(|info| info.get("model")))
                                .and_then(Value::as_str)
                                .filter(|s| !s.is_empty())
                                .map(ToOwned::to_owned);
                        }
                    }
                }
                _ => {}
            }
        }

        tracker.process_line(&line);
    }

    let now = unix_now();
    let snap = tracker.snapshot();
    let session_id = session_id
        .or(snap.session_id)
        .or_else(|| session_id_from_codex_log_path(path))
        .unwrap_or_else(|| path.to_string_lossy().into_owned());
    let project_dir = cwd
        .as_deref()
        .map(codex_project_dir_name)
        .unwrap_or_else(|| "codex".to_string());
    let title = first_user_prompt
        .or(first_developer_prompt)
        .unwrap_or_default();

    Ok(ParsedSession {
        session_id,
        project_dir,
        agent: "codex".to_string(),
        model: model.or(snap.model),
        title,
        total_input_tokens: snap.usage.input_tokens,
        total_output_tokens: snap.usage.output_tokens,
        turn_count: snap.turns,
        created_at: first_ts.unwrap_or(now),
        updated_at: last_ts.unwrap_or(now),
    })
}

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

fn extract_codex_response_text(content: Option<&Value>) -> String {
    let Some(content) = content else {
        return String::new();
    };

    match content {
        Value::String(s) => s.clone(),
        Value::Array(items) => items
            .iter()
            .filter_map(|item| match item.get("type").and_then(Value::as_str) {
                Some("input_text") | Some("text") => {
                    item.get("text").and_then(Value::as_str).map(str::trim)
                }
                _ => None,
            })
            .find(|text| !text.is_empty())
            .map(ToOwned::to_owned)
            .unwrap_or_default(),
        _ => String::new(),
    }
}

fn walk_codex_rollout_files(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    walk_codex_rollout_files_inner(root, &mut files);
    files.sort();
    files
}

fn walk_codex_rollout_files_inner(root: &Path, files: &mut Vec<PathBuf>) {
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk_codex_rollout_files_inner(&path, files);
            continue;
        }

        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if name.starts_with("rollout-") && name.ends_with(".jsonl") {
            files.push(path);
        }
    }
}

fn session_id_from_codex_log_path(path: &Path) -> Option<String> {
    let stem = path.file_stem()?.to_str()?;
    stem.rsplit_once('-').map(|(_, suffix)| suffix.to_string())
}

fn codex_project_dir_name(cwd: &str) -> String {
    let trimmed = cwd.trim();
    if trimmed.is_empty() {
        return "codex".to_string();
    }

    let mut project = String::with_capacity(trimmed.len());
    for ch in trimmed.chars() {
        match ch {
            '/' | '\\' => project.push('-'),
            _ => project.push(ch),
        }
    }
    project
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_temp_jsonl(lines: &[&str]) -> PathBuf {
        let unique = format!(
            "potato-disc-{}-{}.jsonl",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let tmp = std::env::temp_dir().join(unique);
        let mut f = fs::File::create(&tmp).unwrap();
        for line in lines {
            writeln!(f, "{}", line).unwrap();
        }
        tmp
    }

    #[test]
    fn parse_extracts_title_and_tokens() {
        let line_user = r#"{"timestamp":"2024-03-25T10:00:00Z","type":"user","message":{"role":"user","content":"Refactor the auth module"}}"#;
        let line_asst = r#"{"timestamp":"2024-03-25T10:00:05Z","type":"assistant","message":{"role":"assistant","model":"claude-sonnet-4-6","stop_reason":"end_turn","usage":{"input_tokens":50,"output_tokens":80},"content":[]}}"#;

        let tmp = write_temp_jsonl(&[line_user, line_asst]);
        let parsed = parse_claude_jsonl_file(&tmp, "sess-abc", "proj-foo").unwrap();
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

    #[test]
    fn parse_codex_extracts_title_tokens_and_model() {
        let line_meta = r#"{"timestamp":"2026-03-04T00:53:03.666Z","type":"session_meta","payload":{"id":"019cb655-7826-7080-b375-f4bd0ec83d54","cwd":"/Users/test/project","model_provider":"openai"}}"#;
        let line_turn = r#"{"timestamp":"2026-03-04T00:53:03.700Z","type":"turn_context","payload":{"cwd":"/Users/test/project","model":"gpt-5.3-codex"}}"#;
        let line_user = r#"{"timestamp":"2026-03-04T00:53:04.000Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"Implement Codex discovery"}]}}"#;
        let line_assistant = r#"{"timestamp":"2026-03-04T00:53:05.000Z","type":"response_item","payload":{"type":"message","role":"assistant","content":[]}}"#;
        let line_usage = r#"{"timestamp":"2026-03-04T00:53:06.000Z","type":"event_msg","payload":{"type":"turn_completed","usage":{"input_tokens":120,"cached_input_tokens":20,"output_tokens":80}}}"#;

        let tmp = write_temp_jsonl(&[line_meta, line_turn, line_user, line_assistant, line_usage]);
        let parsed = parse_codex_jsonl_file(&tmp).unwrap();
        let _ = fs::remove_file(&tmp);

        assert_eq!(parsed.session_id, "019cb655-7826-7080-b375-f4bd0ec83d54");
        assert_eq!(parsed.project_dir, "-Users-test-project");
        assert_eq!(parsed.agent, "codex");
        assert_eq!(parsed.model.as_deref(), Some("gpt-5.3-codex"));
        assert_eq!(parsed.title, "Implement Codex discovery");
        assert_eq!(parsed.total_input_tokens, 120);
        assert_eq!(parsed.total_output_tokens, 80);
        assert_eq!(parsed.turn_count, 1);
        assert_eq!(parsed.created_at, 1_772_585_583);
        assert_eq!(parsed.updated_at, 1_772_585_586);
    }

    #[test]
    fn parse_codex_title_falls_back_to_developer_when_no_user_message() {
        let line_meta = r#"{"timestamp":"2026-03-04T00:53:03.666Z","type":"session_meta","payload":{"id":"sess-dev-only","cwd":"/tmp/demo"}}"#;
        let line_developer = r#"{"timestamp":"2026-03-04T00:53:04.000Z","type":"response_item","payload":{"type":"message","role":"developer","content":[{"type":"input_text","text":"Developer seed title"}]}}"#;

        let tmp = write_temp_jsonl(&[line_meta, line_developer]);
        let parsed = parse_codex_jsonl_file(&tmp).unwrap();
        let _ = fs::remove_file(&tmp);

        assert_eq!(parsed.title, "Developer seed title");
    }

    #[test]
    fn parse_codex_token_count_history_imports_usage_and_model() {
        let line_meta = r#"{"timestamp":"2026-03-04T00:53:03.666Z","type":"session_meta","payload":{"id":"sess-token-count","cwd":"/Users/test/project"}}"#;
        let line_user = r#"{"timestamp":"2026-03-04T00:53:04.000Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"Import real Codex history"}]}}"#;
        let line_assistant = r#"{"timestamp":"2026-03-04T00:53:05.000Z","type":"response_item","payload":{"type":"message","role":"assistant","content":[]}}"#;
        let line_usage = r#"{"timestamp":"2026-03-04T00:53:06.000Z","type":"event_msg","payload":{"type":"token_count","info":{"model":"gpt-5.3-codex","last_token_usage":{"input_tokens":120,"cached_input_tokens":30,"output_tokens":45}}}}"#;

        let tmp = write_temp_jsonl(&[line_meta, line_user, line_assistant, line_usage]);
        let parsed = parse_codex_jsonl_file(&tmp).unwrap();
        let _ = fs::remove_file(&tmp);

        assert_eq!(parsed.session_id, "sess-token-count");
        assert_eq!(parsed.model.as_deref(), Some("gpt-5.3-codex"));
        assert_eq!(parsed.title, "Import real Codex history");
        assert_eq!(parsed.total_input_tokens, 120);
        assert_eq!(parsed.total_output_tokens, 45);
        assert_eq!(parsed.turn_count, 1);
    }

    #[test]
    fn supported_discovery_providers_are_stable() {
        assert_eq!(
            HistoricalSessionDiscovery::supported(),
            &[
                HistoricalSessionDiscovery::Claude,
                HistoricalSessionDiscovery::Codex,
            ]
        );
    }
}
