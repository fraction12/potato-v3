//! Codex session JSONL log tracker.
//!
//! Codex stores session history under `~/.codex/sessions/YYYY/MM/DD/`.
//! Each session file is named `rollout-<timestamp>-<session-id>.jsonl`.
//!
//! This module provides [`CodexSessionLogTracker`], which tails a session JSONL
//! file and extracts metrics (token usage, tool calls, agent messages) using
//! the same incremental poll approach as [`crate::claude_log::ClaudeSessionLogTracker`].

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use anyhow::Result;
use serde_json::Value;

// ── Types ─────────────────────────────────────────────────────────────────────

/// Status of a Codex tool invocation (command execution).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CodexToolStatus {
    Running,
    Done,
    Error,
}

/// A single tool invocation entry in the Codex sidebar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodexToolEntry {
    pub id: String,
    pub command: String,
    pub status: CodexToolStatus,
    pub output_preview: Option<String>,
    pub exit_code: Option<i64>,
}

/// Cumulative token usage for a Codex session.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CodexUsageTotals {
    pub input_tokens: u64,
    pub cached_input_tokens: u64,
    pub output_tokens: u64,
}

impl CodexUsageTotals {
    /// Total tokens processed (input + cached + output).
    #[must_use]
    pub fn total_tokens(&self) -> u64 {
        self.input_tokens
            .saturating_add(self.cached_input_tokens)
            .saturating_add(self.output_tokens)
    }
}

/// Snapshot of a Codex session suitable for sidebar display.
#[derive(Debug, Clone, Default)]
pub struct CodexSidebarData {
    pub session_id: Option<String>,
    pub model: Option<String>,
    pub turns: u64,
    pub usage: CodexUsageTotals,
    pub tools: Vec<CodexToolEntry>,
    /// First user message (used as session title).
    pub title: String,
}

// ── Internal slot ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
struct ToolSlot {
    order: u64,
    entry: CodexToolEntry,
}

// ── CodexSessionLogTracker ────────────────────────────────────────────────────

/// Incremental JSONL tracker for a Codex session log.
///
/// Call [`poll`] periodically to read new lines and update internal state.
/// Call [`snapshot`] to obtain the latest [`CodexSidebarData`] for display.
#[derive(Debug, Default)]
pub struct CodexSessionLogTracker {
    path: PathBuf,
    offset: u64,
    carry: Vec<u8>,
    next_order: u64,
    session_id: Option<String>,
    model: Option<String>,
    turns: u64,
    usage: CodexUsageTotals,
    tools: BTreeMap<u64, ToolSlot>,
    /// Map from item id → order, for result correlation.
    item_id_to_order: std::collections::HashMap<String, u64>,
    title: String,
}

impl CodexSessionLogTracker {
    /// Create a tracker for the JSONL file at `path`.
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            ..Self::default()
        }
    }

    /// Path to the tracked JSONL file.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Poll for new data in the JSONL file.
    ///
    /// Returns `Ok(true)` if any state changed, `Ok(false)` if nothing new.
    pub fn poll(&mut self) -> Result<bool> {
        if !self.path.exists() {
            return Ok(false);
        }

        let mut file = File::open(&self.path)?;
        // Detect log rotation/truncation: if file shrank, reset to beginning.
        let file_len = file.metadata().map(|m| m.len()).unwrap_or(0);
        if file_len < self.offset {
            self.offset = 0;
            self.carry.clear();
        }
        file.seek(SeekFrom::Start(self.offset))?;

        let mut buf = Vec::new();
        file.read_to_end(&mut buf)?;
        if buf.is_empty() {
            return Ok(false);
        }
        self.offset += buf.len() as u64;

        if !self.carry.is_empty() {
            let mut joined = std::mem::take(&mut self.carry);
            joined.extend_from_slice(&buf);
            buf = joined;
        }

        let mut changed = false;
        let mut start = 0usize;
        for i in 0..buf.len() {
            if buf[i] == b'\n' {
                let line = &buf[start..i];
                if !line.is_empty() && self.process_line_bytes(line) {
                    changed = true;
                }
                start = i + 1;
            }
        }

        if start < buf.len() {
            self.carry = buf[start..].to_vec();
        }

        Ok(changed)
    }

    /// Return the latest snapshot of session data.
    #[must_use]
    pub fn snapshot(&self) -> CodexSidebarData {
        let tools = self.tools.values().map(|s| s.entry.clone()).collect();
        CodexSidebarData {
            session_id: self.session_id.clone(),
            model: self.model.clone(),
            turns: self.turns,
            usage: self.usage.clone(),
            tools,
            title: self.title.clone(),
        }
    }

    fn process_line_bytes(&mut self, line: &[u8]) -> bool {
        let Ok(text) = std::str::from_utf8(line) else {
            return false;
        };
        self.process_line(text)
    }

    /// Process a single JSONL line.
    ///
    /// Codex session JSONL schema:
    /// - `{"timestamp":"…","type":"session_meta","payload":{"id":"…","model_provider":"…",...}}`
    /// - `{"timestamp":"…","type":"response_item","payload":{"type":"message","role":"user|developer","content":[…]}}`
    /// - `{"timestamp":"…","type":"event_msg","payload":{"type":"task_started","turn_id":"…","model_context_window":…}}`
    /// - `{"timestamp":"…","type":"event_msg","payload":{"type":"turn_completed","usage":{…}}}`
    /// - `{"timestamp":"…","type":"event_msg","payload":{"type":"item_started","item":{"id":"…","type":"command_execution","command":"…"}}}`
    /// - `{"timestamp":"…","type":"event_msg","payload":{"type":"item_completed","item":{"id":"…","type":"command_execution","aggregated_output":"…","exit_code":0}}}`
    pub fn process_line(&mut self, line: &str) -> bool {
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            return false;
        };

        let record_type = v["type"].as_str().unwrap_or("");
        let payload = &v["payload"];
        let mut changed = false;

        match record_type {
            "session_meta" => {
                if let Some(id) = payload["id"].as_str() {
                    if !id.is_empty() {
                        self.session_id = Some(id.to_string());
                        changed = true;
                    }
                }
                // model_provider is "openai" etc — not a specific model name.
            }

            "response_item" => {
                let role = payload["role"].as_str().unwrap_or("");
                let p_type = payload["type"].as_str().unwrap_or("");

                if p_type == "message" {
                    if role == "user" || role == "developer" || role == "assistant" {
                        // Extract first user message as title.
                        if self.title.is_empty() && (role == "user" || role == "developer") {
                            if let Some(content) = payload["content"].as_array() {
                                for item in content {
                                    if item["type"].as_str() == Some("input_text")
                                        || item["type"].as_str() == Some("text")
                                    {
                                        if let Some(text) = item["text"].as_str() {
                                            if !text.is_empty() {
                                                self.title = truncate_str(text, 80);
                                                changed = true;
                                            }
                                        }
                                    }
                                }
                            }
                        }

                        if role == "assistant" {
                            self.turns = self.turns.saturating_add(1);
                            changed = true;
                        }
                    }
                }
            }

            "event_msg" => {
                let p_type = payload["type"].as_str().unwrap_or("");

                match p_type {
                    "turn_completed" => {
                        if let Some(usage) = payload.get("usage") {
                            if self.apply_usage(usage) {
                                changed = true;
                            }
                        }
                    }

                    "item_started" => {
                        let item = &payload["item"];
                        if item["type"].as_str() == Some("command_execution") {
                            let id = item["id"].as_str().unwrap_or("").to_string();
                            let command = item["command"].as_str().unwrap_or("").to_string();
                            if !id.is_empty() {
                                let order = self.next_order;
                                self.next_order = self.next_order.saturating_add(1);
                                self.item_id_to_order.insert(id.clone(), order);
                                self.tools.insert(
                                    order,
                                    ToolSlot {
                                        order,
                                        entry: CodexToolEntry {
                                            id,
                                            command,
                                            status: CodexToolStatus::Running,
                                            output_preview: None,
                                            exit_code: None,
                                        },
                                    },
                                );
                                changed = true;
                            }
                        }
                    }

                    "item_completed" => {
                        let item = &payload["item"];
                        let id = item["id"].as_str().unwrap_or("");
                        if !id.is_empty() {
                            if let Some(&order) = self.item_id_to_order.get(id) {
                                if let Some(slot) = self.tools.get_mut(&order) {
                                    let exit_code = item["exit_code"].as_i64();
                                    slot.entry.status = match exit_code {
                                        Some(0) | None => CodexToolStatus::Done,
                                        Some(_) => CodexToolStatus::Error,
                                    };
                                    slot.entry.exit_code = exit_code;
                                    if let Some(output) = item["aggregated_output"].as_str() {
                                        slot.entry.output_preview = Some(truncate_str(output, 200));
                                    }
                                    changed = true;
                                }
                            }
                        }
                    }

                    _ => {}
                }
            }

            _ => {}
        }

        changed
    }

    fn apply_usage(&mut self, usage: &Value) -> bool {
        let before = self.usage.clone();
        self.usage.input_tokens = self
            .usage
            .input_tokens
            .saturating_add(usage["input_tokens"].as_u64().unwrap_or(0));
        self.usage.cached_input_tokens = self
            .usage
            .cached_input_tokens
            .saturating_add(usage["cached_input_tokens"].as_u64().unwrap_or(0));
        self.usage.output_tokens = self
            .usage
            .output_tokens
            .saturating_add(usage["output_tokens"].as_u64().unwrap_or(0));
        self.usage != before
    }
}

// ── Session file discovery ────────────────────────────────────────────────────

/// Root of Codex session storage: `~/.codex/sessions/`.
pub fn codex_sessions_dir(home: &Path) -> PathBuf {
    home.join(".codex").join("sessions")
}

/// Find the most recent Codex session JSONL file for the given session id.
///
/// Searches `~/.codex/sessions/YYYY/MM/DD/rollout-*-<session_id>.jsonl`.
/// Scans date directories in reverse (most recent first) and returns the first match.
pub fn find_session_log(home: &Path, session_id: &str) -> Option<PathBuf> {
    let base = codex_sessions_dir(home);
    if !base.exists() {
        return None;
    }

    // Collect all date-dirs (YYYY/MM/DD) and sort descending.
    let mut date_dirs: Vec<PathBuf> = Vec::new();
    if let Ok(years) = std::fs::read_dir(&base) {
        for year_entry in years.flatten() {
            let year_path = year_entry.path();
            if year_path.is_dir() {
                if let Ok(months) = std::fs::read_dir(&year_path) {
                    for month_entry in months.flatten() {
                        let month_path = month_entry.path();
                        if month_path.is_dir() {
                            if let Ok(days) = std::fs::read_dir(&month_path) {
                                for day_entry in days.flatten() {
                                    let day_path = day_entry.path();
                                    if day_path.is_dir() {
                                        date_dirs.push(day_path);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    date_dirs.sort_unstable_by(|a, b| b.cmp(a)); // most recent first

    let suffix = format!("-{session_id}.jsonl");
    for dir in &date_dirs {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let name = entry.file_name();
                let name_str = name.to_string_lossy();
                if name_str.starts_with("rollout-") && name_str.ends_with(&suffix) {
                    return Some(entry.path());
                }
            }
        }
    }
    None
}

// ── Helpers ───────────────────────────────────────────────────────────────────

use crate::util::truncate_str;

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn tracker() -> CodexSessionLogTracker {
        CodexSessionLogTracker::default()
    }

    // ── process_line: session_meta ────────────────────────────────────────────

    #[test]
    fn parse_session_meta_extracts_id() {
        let mut t = tracker();
        let line = r#"{"timestamp":"2025-01-01T00:00:00Z","type":"session_meta","payload":{"id":"sess-abc-123","cwd":"/tmp","model_provider":"openai"}}"#;
        assert!(t.process_line(line));
        let snap = t.snapshot();
        assert_eq!(snap.session_id.as_deref(), Some("sess-abc-123"));
    }

    #[test]
    fn parse_session_meta_empty_id_ignored() {
        let mut t = tracker();
        let line = r#"{"timestamp":"2025-01-01T00:00:00Z","type":"session_meta","payload":{"id":"","cwd":"/tmp"}}"#;
        assert!(!t.process_line(line));
        assert!(t.snapshot().session_id.is_none());
    }

    // ── process_line: response_item / user message → title ───────────────────

    #[test]
    fn parse_response_item_user_text_sets_title() {
        let mut t = tracker();
        let line = r#"{"timestamp":"2025-01-01T00:00:00Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"Please fix the bug"}]}}"#;
        assert!(t.process_line(line));
        assert_eq!(t.snapshot().title, "Please fix the bug");
    }

    #[test]
    fn parse_response_item_title_not_overwritten_on_second_user_message() {
        let mut t = tracker();
        let line1 = r#"{"timestamp":"2025-01-01T00:00:00Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"First message"}]}}"#;
        let line2 = r#"{"timestamp":"2025-01-01T00:00:01Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"Second message"}]}}"#;
        t.process_line(line1);
        t.process_line(line2);
        assert_eq!(t.snapshot().title, "First message");
    }

    #[test]
    fn parse_response_item_assistant_increments_turns() {
        let mut t = tracker();
        let line = r#"{"timestamp":"2025-01-01T00:00:00Z","type":"response_item","payload":{"type":"message","role":"assistant","content":[]}}"#;
        assert!(t.process_line(line));
        assert_eq!(t.snapshot().turns, 1);
    }

    // ── process_line: event_msg turn_completed → usage ────────────────────────

    #[test]
    fn parse_event_msg_turn_completed_usage() {
        let mut t = tracker();
        let line = r#"{"timestamp":"2025-01-01T00:00:00Z","type":"event_msg","payload":{"type":"turn_completed","usage":{"input_tokens":100,"cached_input_tokens":50,"output_tokens":200}}}"#;
        assert!(t.process_line(line));
        let snap = t.snapshot();
        assert_eq!(snap.usage.input_tokens, 100);
        assert_eq!(snap.usage.cached_input_tokens, 50);
        assert_eq!(snap.usage.output_tokens, 200);
        assert_eq!(snap.usage.total_tokens(), 350);
    }

    #[test]
    fn parse_event_msg_usage_accumulates() {
        let mut t = tracker();
        let line1 = r#"{"timestamp":"2025-01-01T00:00:00Z","type":"event_msg","payload":{"type":"turn_completed","usage":{"input_tokens":10,"cached_input_tokens":0,"output_tokens":20}}}"#;
        let line2 = r#"{"timestamp":"2025-01-01T00:00:01Z","type":"event_msg","payload":{"type":"turn_completed","usage":{"input_tokens":5,"cached_input_tokens":0,"output_tokens":8}}}"#;
        t.process_line(line1);
        t.process_line(line2);
        let snap = t.snapshot();
        assert_eq!(snap.usage.input_tokens, 15);
        assert_eq!(snap.usage.output_tokens, 28);
    }

    // ── process_line: event_msg item_started ──────────────────────────────────

    #[test]
    fn parse_event_msg_item_started_command_execution() {
        let mut t = tracker();
        let line = r#"{"timestamp":"2025-01-01T00:00:00Z","type":"event_msg","payload":{"type":"item_started","item":{"id":"item_0","type":"command_execution","command":"ls -la","aggregated_output":"","exit_code":null,"status":"in_progress"}}}"#;
        assert!(t.process_line(line));
        let snap = t.snapshot();
        assert_eq!(snap.tools.len(), 1);
        assert_eq!(snap.tools[0].id, "item_0");
        assert_eq!(snap.tools[0].command, "ls -la");
        assert_eq!(snap.tools[0].status, CodexToolStatus::Running);
    }

    // ── process_line: event_msg item_completed ────────────────────────────────

    #[test]
    fn parse_event_msg_item_completed_success() {
        let mut t = tracker();
        // First start it.
        let started = r#"{"timestamp":"2025-01-01T00:00:00Z","type":"event_msg","payload":{"type":"item_started","item":{"id":"item_0","type":"command_execution","command":"echo hi","aggregated_output":"","exit_code":null,"status":"in_progress"}}}"#;
        let completed = r#"{"timestamp":"2025-01-01T00:00:01Z","type":"event_msg","payload":{"type":"item_completed","item":{"id":"item_0","type":"command_execution","command":"echo hi","aggregated_output":"hi\n","exit_code":0,"status":"completed"}}}"#;
        t.process_line(started);
        assert!(t.process_line(completed));
        let snap = t.snapshot();
        assert_eq!(snap.tools[0].status, CodexToolStatus::Done);
        assert_eq!(snap.tools[0].exit_code, Some(0));
        assert_eq!(snap.tools[0].output_preview.as_deref(), Some("hi\n"));
    }

    #[test]
    fn parse_event_msg_item_completed_failure() {
        let mut t = tracker();
        let started = r#"{"timestamp":"2025-01-01T00:00:00Z","type":"event_msg","payload":{"type":"item_started","item":{"id":"item_1","type":"command_execution","command":"false","aggregated_output":"","exit_code":null,"status":"in_progress"}}}"#;
        let completed = r#"{"timestamp":"2025-01-01T00:00:01Z","type":"event_msg","payload":{"type":"item_completed","item":{"id":"item_1","type":"command_execution","command":"false","aggregated_output":"","exit_code":1,"status":"completed"}}}"#;
        t.process_line(started);
        assert!(t.process_line(completed));
        let snap = t.snapshot();
        assert_eq!(snap.tools[0].status, CodexToolStatus::Error);
        assert_eq!(snap.tools[0].exit_code, Some(1));
    }

    // ── snapshot default ──────────────────────────────────────────────────────

    #[test]
    fn snapshot_defaults_are_empty() {
        let t = tracker();
        let snap = t.snapshot();
        assert!(snap.session_id.is_none());
        assert!(snap.model.is_none());
        assert_eq!(snap.turns, 0);
        assert!(snap.tools.is_empty());
        assert!(snap.title.is_empty());
        assert_eq!(snap.usage.total_tokens(), 0);
    }

    // ── codex_sessions_dir ────────────────────────────────────────────────────

    #[test]
    fn codex_sessions_dir_path() {
        let home = Path::new("/Users/tester");
        let dir = codex_sessions_dir(home);
        assert_eq!(dir, PathBuf::from("/Users/tester/.codex/sessions"));
    }

    // ── poll: partial lines ───────────────────────────────────────────────────

    #[test]
    fn poll_handles_partial_lines() {
        let tmp =
            std::env::temp_dir().join(format!("potato-codex-log-{}.jsonl", std::process::id()));
        let _ = std::fs::remove_file(&tmp);
        // Write partial line (no trailing newline).
        std::fs::write(
            &tmp,
            b"{\"timestamp\":\"2025-01-01T00:00:00Z\",\"type\":\"session_meta\",\"payload\":{\"id\":\"partial",
        )
        .unwrap();

        let mut t = CodexSessionLogTracker::new(tmp.clone());
        assert!(!t.poll().unwrap());
        assert!(t.snapshot().session_id.is_none());

        // Complete the line.
        std::fs::OpenOptions::new()
            .append(true)
            .open(&tmp)
            .unwrap()
            .write_all(b"\"}}\n")
            .unwrap();

        assert!(t.poll().unwrap());
        assert_eq!(t.snapshot().session_id.as_deref(), Some("partial"));

        let _ = std::fs::remove_file(&tmp);
    }

    // ── truncate_str ──────────────────────────────────────────────────────────

    #[test]
    fn truncate_str_short_unchanged() {
        assert_eq!(truncate_str("hello", 80), "hello");
    }

    #[test]
    fn truncate_str_long_truncated_with_ellipsis() {
        let s = "a".repeat(100);
        let result = truncate_str(&s, 10);
        assert!(result.ends_with('…'));
        assert!(result.chars().count() <= 10);
    }
}
