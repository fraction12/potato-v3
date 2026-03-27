//! List-directory tool — returns a formatted directory listing with sizes and types.

use anyhow::{Context, Result};
use async_trait::async_trait;
use serde_json::{Value, json};
use tokio::fs;

use crate::tools::Tool;

/// Lists the contents of a directory, showing file sizes and entry types.
#[derive(Debug, Default)]
pub struct ListDirTool;

#[async_trait]
impl Tool for ListDirTool {
    fn name(&self) -> &str {
        "list_dir"
    }

    fn description(&self) -> &str {
        "List files and subdirectories at the given path with file sizes and types."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Directory path to list (defaults to current directory)."
                }
            },
            "required": []
        })
    }

    async fn execute(&self, args: Value) -> Result<String> {
        let path = args["path"].as_str().unwrap_or(".");

        let mut read_dir = fs::read_dir(path)
            .await
            .with_context(|| format!("failed to read directory: {path}"))?;

        let mut entries: Vec<(String, String, u64)> = Vec::new();

        while let Some(entry) = read_dir.next_entry().await.context("failed to read entry")? {
            let name = entry.file_name().to_string_lossy().to_string();
            let meta = entry.metadata().await.context("failed to read metadata")?;

            let (kind, size) = if meta.is_dir() {
                ("dir ".to_string(), 0u64)
            } else if meta.is_symlink() {
                ("link".to_string(), meta.len())
            } else {
                ("file".to_string(), meta.len())
            };

            entries.push((kind, name, size));
        }

        // Sort: directories first, then alphabetically by name.
        entries.sort_by(|a, b| {
            let dir_a = a.0 == "dir ";
            let dir_b = b.0 == "dir ";
            dir_b.cmp(&dir_a).then(a.1.cmp(&b.1))
        });

        if entries.is_empty() {
            return Ok(format!("{} (empty)", path));
        }

        let mut lines = vec![format!("{}:", path)];
        for (kind, name, size) in &entries {
            if kind == "dir " {
                lines.push(format!("  [{}]  {}/", kind, name));
            } else {
                lines.push(format!("  [{}]  {}  ({})", kind, name, format_size(*size)));
            }
        }

        Ok(lines.join("\n"))
    }
}

/// Format a byte count into a human-readable string (B, KB, MB, GB).
fn format_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * KB;
    const GB: u64 = 1024 * MB;

    if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::path::PathBuf;

    fn tmp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("potato_listdir_test_{}", name));
        std::fs::create_dir_all(&dir).expect("create dir");
        dir
    }

    #[test]
    fn test_list_dir_name() {
        let tool = ListDirTool;
        assert_eq!(tool.name(), "list_dir");
    }

    #[tokio::test]
    async fn test_list_dir_shows_entries() {
        let dir = tmp_dir("shows_entries");
        std::fs::write(dir.join("alpha.txt"), "content").expect("write alpha");
        std::fs::write(dir.join("beta.rs"), "fn main(){}").expect("write beta");
        let sub = dir.join("subdir");
        std::fs::create_dir_all(&sub).expect("create subdir");

        let tool = ListDirTool;
        let result = tool.execute(json!({
            "path": dir.to_str().unwrap()
        })).await.expect("list_dir should succeed");

        assert!(result.contains("alpha.txt"), "should list alpha.txt");
        assert!(result.contains("beta.rs"), "should list beta.rs");
        assert!(result.contains("subdir"), "should list subdir");
    }
}
