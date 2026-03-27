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

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::path::PathBuf;

    fn tmp_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("potato_write_test_{}", name))
    }

    #[test]
    fn test_write_requires_approval() {
        let tool = WriteFileTool;
        assert!(tool.requires_approval());
    }

    #[tokio::test]
    async fn test_write_creates_file() {
        let path = tmp_path("creates_file.txt");
        // Ensure it doesn't exist yet.
        let _ = tokio::fs::remove_file(&path).await;
        let tool = WriteFileTool;
        let result = tool.execute(json!({
            "path": path.to_str().unwrap(),
            "content": "hello, potato!"
        })).await.expect("write should succeed");
        assert!(result.contains("bytes"));
        let content = tokio::fs::read_to_string(&path).await.expect("read back");
        assert_eq!(content, "hello, potato!");
    }

    #[tokio::test]
    async fn test_write_creates_parent_dirs() {
        let base = tmp_path("nested_dir");
        let path = base.join("sub").join("deep").join("file.txt");
        // Ensure clean state.
        let _ = tokio::fs::remove_dir_all(&base).await;
        let tool = WriteFileTool;
        let result = tool.execute(json!({
            "path": path.to_str().unwrap(),
            "content": "nested content"
        })).await.expect("write nested should succeed");
        assert!(result.contains("bytes"));
        assert!(path.exists());
    }
}
