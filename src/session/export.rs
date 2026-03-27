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
        out.push_str(&msg.content);
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

    let json = serde_json::to_string_pretty(&records)
        .context("failed to serialize messages to JSON")?;

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

/// Capitalize the first character of a string.
fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        None => String::new(),
        Some(f) => f.to_uppercase().to_string() + c.as_str(),
    }
}
