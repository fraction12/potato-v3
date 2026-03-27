//! Shell tool — executes arbitrary shell commands (requires approval).

use std::time::Duration;

use anyhow::{Context, Result};
use async_trait::async_trait;
use serde_json::{Value, json};
use tokio::process::Command;

use crate::tools::Tool;

/// Runs a shell command and returns its combined stdout + stderr.
///
/// Always requires user approval before execution.
#[derive(Debug, Default)]
pub struct ShellTool;

#[async_trait]
impl Tool for ShellTool {
    fn name(&self) -> &str {
        "shell"
    }

    fn description(&self) -> &str {
        "Execute a shell command and return its combined stdout + stderr output."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "Shell command to execute."
                },
                "working_dir": {
                    "type": "string",
                    "description": "Optional working directory for the command."
                },
                "timeout_secs": {
                    "type": "integer",
                    "description": "Optional timeout in seconds (default: 30)."
                }
            },
            "required": ["command"]
        })
    }

    async fn execute(&self, args: Value) -> Result<String> {
        let command = args["command"]
            .as_str()
            .context("missing required parameter: command")?
            .to_string();

        let working_dir = args["working_dir"].as_str().map(|s| s.to_string());

        let timeout_secs = args["timeout_secs"]
            .as_u64()
            .unwrap_or(30);

        let timeout = Duration::from_secs(timeout_secs);

        // Build the command, running via sh -c for shell expansion.
        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg(&command);
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());

        if let Some(dir) = working_dir {
            cmd.current_dir(&dir);
        }

        // Spawn and wait with timeout.
        let child = cmd.spawn().context("failed to spawn shell process")?;

        let output = tokio::time::timeout(timeout, child.wait_with_output())
            .await
            .context("command timed out")?
            .context("failed to wait for command")?;

        let mut result = String::new();

        let stdout = String::from_utf8_lossy(&output.stdout);
        if !stdout.is_empty() {
            result.push_str(&stdout);
        }

        let stderr = String::from_utf8_lossy(&output.stderr);
        if !stderr.is_empty() {
            if !result.is_empty() {
                result.push('\n');
            }
            result.push_str("[stderr]\n");
            result.push_str(&stderr);
        }

        if !output.status.success() {
            let code = output.status.code().unwrap_or(-1);
            if !result.is_empty() {
                result.push('\n');
            }
            result.push_str(&format!("[exit code: {}]", code));
        }

        Ok(result)
    }

    fn requires_approval(&self) -> bool {
        true
    }
}
