//! Session export — serialize session history to Markdown or JSON.

use anyhow::Result;

use crate::ollama::types::ChatMessage;

/// Export a list of messages to a Markdown string.
///
/// Each message becomes a headed section: `## User` / `## Assistant`.
pub fn export_markdown(messages: &[ChatMessage]) -> String {
    let mut out = String::new();
    for msg in messages {
        let heading = match msg.role.as_str() {
            "user" => "## User",
            "assistant" => "## Assistant",
            other => &format!("## {}", other),
        };
        out.push_str(heading);
        out.push('\n');
        out.push('\n');
        out.push_str(&msg.content);
        out.push_str("\n\n");
    }
    out
}

/// Export a list of messages to a pretty-printed JSON string.
pub fn export_json(messages: &[ChatMessage]) -> Result<String> {
    Ok(serde_json::to_string_pretty(messages)?)
}
