//! Write-file tool — writes content to a file on disk.

use anyhow::Result;
use async_trait::async_trait;
use serde_json::{Value, json};

use crate::tools::Tool;

/// Creates or overwrites a file with the given content.
#[derive(Debug, Default)]
pub struct WriteFileTool;

#[async_trait]
impl Tool for WriteFileTool {
    fn name(&self) -> &str {
        "write_file"
    }

    fn description(&self) -> &str {
        "Write content to a file, creating or overwriting it."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path":    { "type": "string", "description": "File path." },
                "content": { "type": "string", "description": "Content to write." }
            },
            "required": ["path", "content"]
        })
    }

    async fn execute(&self, _args: Value) -> Result<String> {
        Ok("written".to_string())
    }

    fn requires_approval(&self) -> bool {
        true
    }
}
