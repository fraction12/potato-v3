use std::collections::{BTreeMap, HashMap};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use anyhow::Result;
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClaudeToolStatus {
    Running,
    Done,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaudeToolEntry {
    pub id: String,
    pub name: String,
    pub status: ClaudeToolStatus,
    pub input_preview: String,
    pub result_preview: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ClaudeUsageTotals {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_creation_input_tokens: u64,
    pub cache_read_input_tokens: u64,
    pub web_search_requests: u64,
    pub web_fetch_requests: u64,
}

impl ClaudeUsageTotals {
    pub fn total_tokens(&self) -> u64 {
        self.input_tokens + self.output_tokens
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ClaudeSidebarData {
    pub model: Option<String>,
    pub turns: u64,
    pub last_stop_reason: Option<String>,
    pub usage: ClaudeUsageTotals,
    pub tools: Vec<ClaudeToolEntry>,
    /// First user prompt text (used as session title in the rail).
    pub title: String,
}

#[derive(Debug, Clone)]
struct ToolSlot {
    order: u64,
    entry: ClaudeToolEntry,
}

#[derive(Debug, Default)]
pub struct ClaudeSessionLogTracker {
    path: PathBuf,
    offset: u64,
    carry: Vec<u8>,
    next_order: u64,
    title: String,
    model: Option<String>,
    turns: u64,
    last_stop_reason: Option<String>,
    usage: ClaudeUsageTotals,
    tools: HashMap<String, ToolSlot>,
}

impl ClaudeSessionLogTracker {
    pub fn new(path: PathBuf) -> Self {
        Self { path, ..Self::default() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn poll(&mut self) -> Result<bool> {
        if !self.path.exists() {
            return Ok(false);
        }

        let mut file = File::open(&self.path)?;
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

    pub fn snapshot(&self) -> ClaudeSidebarData {
        let mut ordered: BTreeMap<u64, ClaudeToolEntry> = BTreeMap::new();
        for slot in self.tools.values() {
            ordered.insert(slot.order, slot.entry.clone());
        }

        ClaudeSidebarData {
            model: self.model.clone(),
            turns: self.turns,
            last_stop_reason: self.last_stop_reason.clone(),
            usage: self.usage.clone(),
            tools: ordered.into_values().collect(),
            title: self.title.clone(),
        }
    }

    fn process_line_bytes(&mut self, line: &[u8]) -> bool {
        let Ok(text) = std::str::from_utf8(line) else {
            return false;
        };
        self.process_line(text)
    }

    pub fn process_line(&mut self, line: &str) -> bool {
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            return false;
        };

        let Some(message) = v.get("message") else {
            return false;
        };

        let role = message.get("role").and_then(Value::as_str).unwrap_or_default();
        let mut changed = false;

        if role == "assistant" {
            if let Some(model) = message.get("model").and_then(Value::as_str) {
                if !model.is_empty() && model != "<synthetic>" {
                    self.model = Some(model.to_string());
                }
            }

            if let Some(stop_reason) = message.get("stop_reason").and_then(Value::as_str) {
                if !stop_reason.is_empty() {
                    self.last_stop_reason = Some(stop_reason.to_string());
                }
            }

            self.apply_usage(message.get("usage"));

            self.turns = self.turns.saturating_add(1);
            // Any assistant message is a change.
            changed = true;

            if let Some(content) = message.get("content").and_then(Value::as_array) {
                for item in content {
                    if item.get("type").and_then(Value::as_str) == Some("tool_use") {
                        let id = item.get("id").and_then(Value::as_str).unwrap_or_default();
                        let name = item.get("name").and_then(Value::as_str).unwrap_or("tool");
                        let input_preview = compact_json(item.get("input"));
                        self.upsert_tool_start(id, name, input_preview);
                        changed = true;
                    }
                }
            }
        } else if role == "user" {
            // Extract first user prompt as session title.
            if self.title.is_empty() {
                if let Some(content) = message.get("content") {
                    let text = extract_user_text(content);
                    if !text.is_empty() {
                        self.title = truncate_str(&text, 80);
                        changed = true;
                    }
                }
            }
            if let Some(content) = message.get("content").and_then(Value::as_array) {
                for item in content {
                    if item.get("type").and_then(Value::as_str) == Some("tool_result") {
                        let id = item
                            .get("tool_use_id")
                            .and_then(Value::as_str)
                            .unwrap_or_default();
                        let is_error = item
                            .get("is_error")
                            .and_then(Value::as_bool)
                            .or_else(|| item.get("isError").and_then(Value::as_bool))
                            .unwrap_or(false);
                        let preview = compact_json(item.get("content"));
                        self.upsert_tool_result(id, preview, is_error);
                        changed = true;
                    }
                }
            }
        }

        changed
    }

    fn apply_usage(&mut self, usage: Option<&Value>) -> bool {
        let Some(usage) = usage else { return false; };
        let before = self.usage.clone();

        self.usage.input_tokens = self
            .usage
            .input_tokens
            .saturating_add(usage.get("input_tokens").and_then(Value::as_u64).unwrap_or(0));
        self.usage.output_tokens = self
            .usage
            .output_tokens
            .saturating_add(usage.get("output_tokens").and_then(Value::as_u64).unwrap_or(0));
        self.usage.cache_creation_input_tokens = self.usage.cache_creation_input_tokens.saturating_add(
            usage.get("cache_creation_input_tokens").and_then(Value::as_u64).unwrap_or(0),
        );
        self.usage.cache_read_input_tokens = self.usage.cache_read_input_tokens.saturating_add(
            usage.get("cache_read_input_tokens").and_then(Value::as_u64).unwrap_or(0),
        );

        if let Some(server_tool_use) = usage.get("server_tool_use") {
            self.usage.web_search_requests = self.usage.web_search_requests.saturating_add(
                server_tool_use
                    .get("web_search_requests")
                    .and_then(Value::as_u64)
                    .unwrap_or(0),
            );
            self.usage.web_fetch_requests = self.usage.web_fetch_requests.saturating_add(
                server_tool_use
                    .get("web_fetch_requests")
                    .and_then(Value::as_u64)
                    .unwrap_or(0),
            );
        }

        self.usage != before
    }

    fn upsert_tool_start(&mut self, id: &str, name: &str, input_preview: String) {
        if id.is_empty() {
            return;
        }
        let order = self.next_order;
        self.next_order = self.next_order.saturating_add(1);
        self.tools.entry(id.to_string()).or_insert_with(|| ToolSlot {
            order,
            entry: ClaudeToolEntry {
                id: id.to_string(),
                name: name.to_string(),
                status: ClaudeToolStatus::Running,
                input_preview,
                result_preview: None,
            },
        });
    }

    fn upsert_tool_result(&mut self, id: &str, result_preview: String, is_error: bool) {
        if id.is_empty() {
            return;
        }
        if let Some(slot) = self.tools.get_mut(id) {
            slot.entry.status = if is_error {
                ClaudeToolStatus::Error
            } else {
                ClaudeToolStatus::Done
            };
            slot.entry.result_preview = Some(result_preview);
            return;
        }

        let order = self.next_order;
        self.next_order = self.next_order.saturating_add(1);
        self.tools.insert(
            id.to_string(),
            ToolSlot {
                order,
                entry: ClaudeToolEntry {
                    id: id.to_string(),
                    name: "tool".to_string(),
                    status: if is_error {
                        ClaudeToolStatus::Error
                    } else {
                        ClaudeToolStatus::Done
                    },
                    input_preview: String::new(),
                    result_preview: Some(result_preview),
                },
            },
        );
    }
}

pub fn claude_projects_dir(home: &Path) -> PathBuf {
    home.join(".claude").join("projects")
}

pub fn project_dir_name(cwd: &Path) -> String {
    let raw = cwd.to_string_lossy();
    let mut result = String::with_capacity(raw.len());
    for c in raw.chars() {
        if c.is_ascii_alphanumeric() || c == '-' {
            result.push(c);
        } else {
            result.push('-');
        }
    }
    result
}

pub fn session_log_path(home: &Path, cwd: &Path, session_id: &str) -> PathBuf {
    claude_projects_dir(home)
        .join(project_dir_name(cwd))
        .join(format!("{session_id}.jsonl"))
}

/// Extract plain text from a Claude user message `content` field.
/// Content may be a string or an array of objects with `type: "text"`.
fn extract_user_text(content: &Value) -> String {
    match content {
        Value::String(s) => s.clone(),
        Value::Array(arr) => {
            for item in arr {
                if item.get("type").and_then(Value::as_str) == Some("text") {
                    if let Some(text) = item.get("text").and_then(Value::as_str) {
                        return text.to_string();
                    }
                }
                // Also handle tool_result items — skip those for title extraction.
            }
            String::new()
        }
        _ => String::new(),
    }
}

/// Truncate a string to at most `max` characters.
fn truncate_str(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(max.saturating_sub(1)).collect();
        format!("{}…", truncated)
    }
}

fn compact_json(value: Option<&Value>) -> String {
    let Some(value) = value else { return String::new(); };
    let raw = match value {
        Value::String(s) => s.clone(),
        other => serde_json::to_string(other).unwrap_or_default(),
    };
    let compact = raw.replace('\n', " ").split_whitespace().collect::<Vec<_>>().join(" ");
    if compact.len() > 120 {
        // Find a char boundary at or before byte 119.
        let mut end = 119;
        while end > 0 && !compact.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}…", &compact[..end])
    } else {
        compact
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;

    #[test]
    fn parses_assistant_usage_and_tool_use() {
        let mut tracker = ClaudeSessionLogTracker::default();
        let line = r#"{"type":"assistant","message":{"role":"assistant","model":"claude-sonnet-4-6","stop_reason":"tool_use","usage":{"input_tokens":10,"output_tokens":20,"cache_creation_input_tokens":30,"cache_read_input_tokens":40,"server_tool_use":{"web_search_requests":2,"web_fetch_requests":3}},"content":[{"type":"tool_use","id":"toolu_1","name":"Read","input":{"file_path":"README.md"}}]}}"#;

        assert!(tracker.process_line(line));

        let snap = tracker.snapshot();
        assert_eq!(snap.model.as_deref(), Some("claude-sonnet-4-6"));
        assert_eq!(snap.turns, 1);
        assert_eq!(snap.last_stop_reason.as_deref(), Some("tool_use"));
        assert_eq!(snap.usage.input_tokens, 10);
        assert_eq!(snap.usage.output_tokens, 20);
        assert_eq!(snap.usage.cache_creation_input_tokens, 30);
        assert_eq!(snap.usage.cache_read_input_tokens, 40);
        assert_eq!(snap.usage.web_search_requests, 2);
        assert_eq!(snap.usage.web_fetch_requests, 3);
        assert_eq!(snap.tools.len(), 1);
        assert_eq!(snap.tools[0].name, "Read");
        assert_eq!(snap.tools[0].status, ClaudeToolStatus::Running);
        assert!(snap.tools[0].input_preview.contains("README.md"));
    }

    #[test]
    fn matches_tool_result_back_to_tool() {
        let mut tracker = ClaudeSessionLogTracker::default();
        tracker.process_line(r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"toolu_1","name":"Bash","input":{"command":"ls"}}]}}"#);
        tracker.process_line(r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_1","content":"ok","is_error":false}]}}"#);

        let snap = tracker.snapshot();
        assert_eq!(snap.tools.len(), 1);
        assert_eq!(snap.tools[0].status, ClaudeToolStatus::Done);
        assert_eq!(snap.tools[0].result_preview.as_deref(), Some("ok"));
    }

    #[test]
    fn marks_tool_errors_from_claude_log() {
        let mut tracker = ClaudeSessionLogTracker::default();
        tracker.process_line(r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_x","content":"boom","is_error":true}]}}"#);

        let snap = tracker.snapshot();
        assert_eq!(snap.tools.len(), 1);
        assert_eq!(snap.tools[0].status, ClaudeToolStatus::Error);
        assert_eq!(snap.tools[0].result_preview.as_deref(), Some("boom"));
    }

    #[test]
    fn session_log_path_uses_claude_project_layout() {
        let home = Path::new("/Users/tester");
        let cwd = Path::new("/Users/tester/Documents/Projects/potato-v3");
        let path = session_log_path(home, cwd, "abc-123");
        assert_eq!(
            path,
            PathBuf::from("/Users/tester/.claude/projects/-Users-tester-Documents-Projects-potato-v3/abc-123.jsonl")
        );
    }

    #[test]
    fn project_dir_name_replaces_underscores_and_slashes() {
        let cwd = Path::new("/Users/dushyant_jarvis/Documents/Projects/potato-v3");
        assert_eq!(
            project_dir_name(cwd),
            "-Users-dushyant-jarvis-Documents-Projects-potato-v3"
        );
    }

    #[test]
    fn project_dir_name_preserves_existing_dashes() {
        let cwd = Path::new("/home/user/my-project");
        assert_eq!(project_dir_name(cwd), "-home-user-my-project");
    }

    #[test]
    fn poll_handles_partial_lines() {
        let tmp = std::env::temp_dir().join(format!("potato-claude-log-{}.jsonl", std::process::id()));
        let _ = fs::remove_file(&tmp);
        fs::write(&tmp, b"{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"usage\":{\"input_tokens\":1}")
            .unwrap();

        let mut tracker = ClaudeSessionLogTracker::new(tmp.clone());
        assert!(!tracker.poll().unwrap());
        assert_eq!(tracker.snapshot().usage.input_tokens, 0);

        fs::OpenOptions::new()
            .append(true)
            .open(&tmp)
            .unwrap()
            .write_all(b",\"content\":[]}}\n")
            .unwrap();

        assert!(tracker.poll().unwrap());
        assert_eq!(tracker.snapshot().usage.input_tokens, 1);

        let _ = fs::remove_file(&tmp);
    }
}
