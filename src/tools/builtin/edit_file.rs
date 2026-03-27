//! Edit-file tool — applies a targeted exact-string replacement to a file.

use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use serde_json::{Value, json};
use tokio::fs;

use crate::tools::Tool;

/// Finds `old_text` in a file and replaces the first occurrence with `new_text`.
///
/// The match must be exact (byte-identical). Always requires user approval.
#[derive(Debug, Default)]
pub struct EditFileTool;

#[async_trait]
impl Tool for EditFileTool {
    fn name(&self) -> &str {
        "edit_file"
    }

    fn description(&self) -> &str {
        "Replace an exact string in a file with new content (first occurrence only)."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the file to edit."
                },
                "old_text": {
                    "type": "string",
                    "description": "Exact text to find in the file."
                },
                "new_text": {
                    "type": "string",
                    "description": "Replacement text."
                }
            },
            "required": ["path", "old_text", "new_text"]
        })
    }

    async fn execute(&self, args: Value) -> Result<String> {
        let path = args["path"]
            .as_str()
            .context("missing required parameter: path")?;

        let old_text = args["old_text"]
            .as_str()
            .context("missing required parameter: old_text")?;

        let new_text = args["new_text"]
            .as_str()
            .context("missing required parameter: new_text")?;

        let original = fs::read_to_string(path)
            .await
            .with_context(|| format!("failed to read file: {path}"))?;

        if !original.contains(old_text) {
            bail!("old_text not found in file: {path}");
        }

        // Replace only the first occurrence to avoid unintended mass-edits.
        let updated = original.replacen(old_text, new_text, 1);

        fs::write(path, &updated)
            .await
            .with_context(|| format!("failed to write file: {path}"))?;

        Ok(format!("successfully edited {}", path))
    }

    fn requires_approval(&self) -> bool {
        true
    }
}
