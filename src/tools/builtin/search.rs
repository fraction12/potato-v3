//! Search tool — grep/ripgrep pattern search across files.

use anyhow::{Context, Result};
use async_trait::async_trait;
use serde_json::{Value, json};
use tokio::process::Command;

use crate::tools::Tool;

/// Searches for a pattern across files using `rg` (ripgrep) if available,
/// falling back to `grep -rn`.
#[derive(Debug, Default)]
pub struct SearchTool;

#[async_trait]
impl Tool for SearchTool {
    fn name(&self) -> &str {
        "search"
    }

    fn description(&self) -> &str {
        "Search for a regex pattern across files. Uses ripgrep if available, otherwise grep."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "pattern": {
                    "type": "string",
                    "description": "Regex pattern to search for."
                },
                "path": {
                    "type": "string",
                    "description": "Directory or file path to search (defaults to current directory)."
                },
                "max_results": {
                    "type": "integer",
                    "description": "Maximum number of matching lines to return (default: 20)."
                }
            },
            "required": ["pattern"]
        })
    }

    async fn execute(&self, args: Value) -> Result<String> {
        let pattern = args["pattern"]
            .as_str()
            .context("missing required parameter: pattern")?;

        let path = args["path"].as_str().unwrap_or(".");

        let max_results = args["max_results"].as_u64().unwrap_or(20) as usize;

        // Prefer ripgrep; fall back to grep.
        let use_rg = which_rg().await;

        let output = if use_rg {
            Command::new("rg")
                .arg("--line-number")
                .arg("--no-heading")
                .arg("--color=never")
                .arg(pattern)
                .arg(path)
                .output()
                .await
                .context("failed to run rg")?
        } else {
            Command::new("grep")
                .arg("-rn")
                .arg("--color=never")
                .arg(pattern)
                .arg(path)
                .output()
                .await
                .context("failed to run grep")?
        };

        let stdout = String::from_utf8_lossy(&output.stdout);

        if stdout.trim().is_empty() {
            return Ok("no matches found".to_string());
        }

        // Limit the number of result lines.
        let lines: Vec<&str> = stdout.lines().take(max_results).collect();
        let mut result = lines.join("\n");

        let total_lines = stdout.lines().count();
        if total_lines > max_results {
            result.push_str(&format!(
                "\n... ({} more results truncated)",
                total_lines - max_results
            ));
        }

        Ok(result)
    }
}

/// Check whether `rg` (ripgrep) is available on the PATH.
async fn which_rg() -> bool {
    Command::new("which")
        .arg("rg")
        .output()
        .await
        .map(|o| o.status.success())
        .unwrap_or(false)
}
