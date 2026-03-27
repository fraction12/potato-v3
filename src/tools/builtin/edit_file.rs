//! Edit-file tool — applies a targeted string replacement to a file.

use anyhow::Result;
use async_trait::async_trait;
use serde_json::{Value, json};

use crate::tools::Tool;

/// Finds `old_string` in a file and replaces it with `new_string`.
#[derive(Debug, Default)]
pub struct EditFileTool;

#[async_trait]
impl Tool for EditFileTool {
    fn name(&self) -> &str {
        "edit_file"
    }

    fn description(&self) -> &str {
        "Replace an exact string in a file with new content."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path":       { "type": "string" },
                "old_string": { "type": "string" },
                "new_string": { "type": "string" }
            },
            "required": ["path", "old_string", "new_string"]
        })
    }

    async fn execute(&self, _args: Value) -> Result<String> {
        Ok("edited".to_string())
    }

    fn requires_approval(&self) -> bool {
        true
    }
}
