//! Write-file tool — writes content to a file on disk, creating parent dirs as needed.

use anyhow::{Context, Result};
use async_trait::async_trait;
use serde_json::{Value, json};
use tokio::fs;

use crate::tools::Tool;

/// Creates or overwrites a file with the given content.
///
/// Parent directories are created automatically. Always requires user approval.
#[derive(Debug, Default)]
pub struct WriteFileTool;

#[async_trait]
impl Tool for WriteFileTool {
    fn name(&self) -> &str {
        "write_file"
    }

    fn description(&self) -> &str {
        "Write content to a file, creating it (and any parent directories) if needed."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "File path to write to."
                },
                "content": {
                    "type": "string",
                    "description": "Content to write to the file."
                }
            },
            "required": ["path", "content"]
        })
    }

    async fn execute(&self, args: Value) -> Result<String> {
        let path = args["path"]
            .as_str()
            .context("missing required parameter: path")?;

        let content = args["content"]
            .as_str()
            .context("missing required parameter: content")?;

        // Create parent directories if they do not exist.
        if let Some(parent) = std::path::Path::new(path).parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent)
                    .await
                    .with_context(|| format!("failed to create parent directories for: {path}"))?;
            }
        }

        fs::write(path, content)
            .await
            .with_context(|| format!("failed to write file: {path}"))?;

        let bytes = content.len();
        Ok(format!("wrote {} bytes to {}", bytes, path))
    }

    fn requires_approval(&self) -> bool {
        true
    }
}
