//! Shell tool — executes arbitrary shell commands (requires approval).

use anyhow::Result;
use async_trait::async_trait;
use serde_json::{Value, json};

use crate::tools::Tool;

/// Runs a shell command and returns its stdout + stderr.
#[derive(Debug, Default)]
pub struct ShellTool;

#[async_trait]
impl Tool for ShellTool {
    fn name(&self) -> &str {
        "shell"
    }

    fn description(&self) -> &str {
        "Execute a shell command and return its output."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "command": { "type": "string", "description": "Shell command to execute." }
            },
            "required": ["command"]
        })
    }

    async fn execute(&self, _args: Value) -> Result<String> {
        Ok(String::new())
    }

    fn requires_approval(&self) -> bool {
        true
    }
}
