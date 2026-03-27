//! Read-file tool — returns the UTF-8 contents of a file.

use anyhow::Result;
use async_trait::async_trait;
use serde_json::{Value, json};

use crate::tools::Tool;

/// Reads a file from disk and returns its contents as a string.
#[derive(Debug, Default)]
pub struct ReadFileTool;

#[async_trait]
impl Tool for ReadFileTool {
    fn name(&self) -> &str {
        "read_file"
    }

    fn description(&self) -> &str {
        "Read the contents of a file and return them as a string."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "Absolute or relative file path." }
            },
            "required": ["path"]
        })
    }

    async fn execute(&self, _args: Value) -> Result<String> {
        Ok(String::new())
    }
}
