//! Read-file tool — returns the UTF-8 contents of a file with optional line slicing.

use anyhow::{Context, Result};
use async_trait::async_trait;
use serde_json::{Value, json};
use tokio::fs;

use crate::tools::Tool;

/// Reads a file from disk and returns its contents as a string.
///
/// Supports `offset` (1-based start line) and `limit` (max lines to return)
/// for working with large files without reading everything into memory.
#[derive(Debug, Default)]
pub struct ReadFileTool;

#[async_trait]
impl Tool for ReadFileTool {
    fn name(&self) -> &str {
        "read_file"
    }

    fn description(&self) -> &str {
        "Read the contents of a file. Supports optional line offset and limit for large files."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Absolute or relative file path."
                },
                "offset": {
                    "type": "integer",
                    "description": "1-based line number to start reading from."
                },
                "limit": {
                    "type": "integer",
                    "description": "Maximum number of lines to return."
                }
            },
            "required": ["path"]
        })
    }

    async fn execute(&self, args: Value) -> Result<String> {
        let path = args["path"]
            .as_str()
            .context("missing required parameter: path")?;

        let raw = fs::read_to_string(path)
            .await
            .with_context(|| format!("failed to read file: {path}"))?;

        // If no slicing requested, return the full content.
        let offset = args["offset"].as_u64();
        let limit = args["limit"].as_u64();

        if offset.is_none() && limit.is_none() {
            return Ok(raw);
        }

        // Apply line-based slicing.
        let lines: Vec<&str> = raw.lines().collect();
        let total = lines.len();

        // offset is 1-based; convert to 0-based index.
        let start = offset
            .map(|o| (o.saturating_sub(1) as usize).min(total))
            .unwrap_or(0);

        let end = limit
            .map(|l| (start + l as usize).min(total))
            .unwrap_or(total);

        let slice = lines[start..end].join("\n");
        Ok(slice)
    }
}
