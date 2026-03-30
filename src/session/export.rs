//! Session export — serialize session history to Markdown or JSON.

use anyhow::{Context, Result};
use tokio::fs;

use super::store::StoredMessage;

/// Write a conversation to a Markdown file at `path`.
///
/// Each message becomes a headed section:
/// `## User` / `## Assistant` / `## System` etc.
pub async fn export_markdown(messages: &[StoredMessage], path: &str) -> Result<()> {
    let mut out = String::new();
    out.push_str("# Potato Session Export\n\n");

    for msg in messages {
        let heading = match msg.role.as_str() {
            "user" => "## User",
            "assistant" => "## Assistant",
            "system" => "## System",
            "tool" => "## Tool",
            other => &format!("## {}", capitalize(other)),
        };
        out.push_str(heading);
        out.push_str("\n\n");
        out.push_str(&escape_markdown_content(&msg.content));
        out.push_str("\n\n");
    }

    write_file(path, &out).await
}

/// Write a conversation to a JSON file at `path` (pretty-printed).
pub async fn export_json(messages: &[StoredMessage], path: &str) -> Result<()> {
    // Convert to a simple serializable form.
    let records: Vec<serde_json::Value> = messages
        .iter()
        .map(|m| {
            serde_json::json!({
                "id": m.id,
                "session_id": m.session_id,
                "role": m.role,
                "content": m.content,
                "created_at": m.created_at,
                "tokens": m.tokens
            })
        })
        .collect();

    let json =
        serde_json::to_string_pretty(&records).context("failed to serialize messages to JSON")?;

    write_file(path, &json).await
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Write a string to a file, creating parent directories as needed.
async fn write_file(path: &str, content: &str) -> Result<()> {
    if let Some(parent) = std::path::Path::new(path).parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)
                .await
                .with_context(|| format!("failed to create directories for: {path}"))?;
        }
    }
    fs::write(path, content)
        .await
        .with_context(|| format!("failed to write export to: {path}"))
}

/// Escape markdown metacharacters in message content that would corrupt
/// the exported document structure (headings and code fences).
fn escape_markdown_content(content: &str) -> String {
    content
        .lines()
        .map(|line| {
            if line.starts_with('#') || line.starts_with("```") {
                format!("\\{line}")
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Capitalize the first character of a string.
fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        None => String::new(),
        Some(f) => f.to_uppercase().to_string() + c.as_str(),
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper to build a `StoredMessage` with sensible defaults.
    fn msg(role: &str, content: &str) -> StoredMessage {
        StoredMessage {
            id: format!("msg-{role}"),
            session_id: "test-session".into(),
            role: role.into(),
            content: content.into(),
            created_at: 1700000000,
            tokens: Some(42),
        }
    }

    #[test]
    fn capitalize_empty_string() {
        assert_eq!(capitalize(""), "");
    }

    #[test]
    fn capitalize_single_char() {
        assert_eq!(capitalize("a"), "A");
    }

    #[test]
    fn capitalize_word() {
        assert_eq!(capitalize("hello"), "Hello");
    }

    #[test]
    fn capitalize_already_upper() {
        assert_eq!(capitalize("Hello"), "Hello");
    }

    #[tokio::test]
    async fn export_markdown_basic() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("out.md");
        let path_str = path.to_str().unwrap();

        let messages = vec![
            msg("user", "What is Potato?"),
            msg("assistant", "A terminal cockpit for coding agents."),
        ];

        export_markdown(&messages, path_str).await.unwrap();

        let content = tokio::fs::read_to_string(&path).await.unwrap();
        assert!(content.starts_with("# Potato Session Export"));
        assert!(content.contains("## User\n\nWhat is Potato?"));
        assert!(content.contains("## Assistant\n\nA terminal cockpit for coding agents."));
    }

    #[tokio::test]
    async fn export_markdown_all_roles() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("roles.md");
        let path_str = path.to_str().unwrap();

        let messages = vec![
            msg("system", "You are helpful."),
            msg("user", "Hi"),
            msg("assistant", "Hello!"),
            msg("tool", "result: 42"),
            msg("observer", "Noted."),
        ];

        export_markdown(&messages, path_str).await.unwrap();

        let content = tokio::fs::read_to_string(&path).await.unwrap();
        assert!(content.contains("## System"));
        assert!(content.contains("## User"));
        assert!(content.contains("## Assistant"));
        assert!(content.contains("## Tool"));
        // Unknown role should be capitalized
        assert!(content.contains("## Observer"));
    }

    #[tokio::test]
    async fn export_markdown_empty_messages() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("empty.md");
        let path_str = path.to_str().unwrap();

        export_markdown(&[], path_str).await.unwrap();

        let content = tokio::fs::read_to_string(&path).await.unwrap();
        assert_eq!(content, "# Potato Session Export\n\n");
    }

    #[tokio::test]
    async fn export_json_basic() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("out.json");
        let path_str = path.to_str().unwrap();

        let messages = vec![msg("user", "Hello"), msg("assistant", "Hi there")];

        export_json(&messages, path_str).await.unwrap();

        let content = tokio::fs::read_to_string(&path).await.unwrap();
        let parsed: Vec<serde_json::Value> = serde_json::from_str(&content).unwrap();
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0]["role"], "user");
        assert_eq!(parsed[0]["content"], "Hello");
        assert_eq!(parsed[0]["tokens"], 42);
        assert_eq!(parsed[1]["role"], "assistant");
        assert_eq!(parsed[1]["session_id"], "test-session");
    }

    #[tokio::test]
    async fn export_json_empty_messages() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("empty.json");
        let path_str = path.to_str().unwrap();

        export_json(&[], path_str).await.unwrap();

        let content = tokio::fs::read_to_string(&path).await.unwrap();
        let parsed: Vec<serde_json::Value> = serde_json::from_str(&content).unwrap();
        assert!(parsed.is_empty());
    }

    #[tokio::test]
    async fn export_json_null_tokens() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("null-tok.json");
        let path_str = path.to_str().unwrap();

        let messages = vec![StoredMessage {
            id: "m1".into(),
            session_id: "s1".into(),
            role: "user".into(),
            content: "test".into(),
            created_at: 1700000000,
            tokens: None,
        }];

        export_json(&messages, path_str).await.unwrap();

        let content = tokio::fs::read_to_string(&path).await.unwrap();
        let parsed: Vec<serde_json::Value> = serde_json::from_str(&content).unwrap();
        assert!(parsed[0]["tokens"].is_null());
    }

    #[tokio::test]
    async fn write_file_creates_nested_dirs() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a").join("b").join("c").join("out.txt");
        let path_str = path.to_str().unwrap();

        write_file(path_str, "hello").await.unwrap();

        let content = tokio::fs::read_to_string(&path).await.unwrap();
        assert_eq!(content, "hello");
    }
}
