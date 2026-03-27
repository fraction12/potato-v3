//! List-directory tool — returns a directory listing.

use anyhow::Result;
use async_trait::async_trait;
use serde_json::{Value, json};

use crate::tools::Tool;

/// Lists files and directories at a given path.
#[derive(Debug, Default)]
pub struct ListDirTool;

#[async_trait]
impl Tool for ListDirTool {
    fn name(&self) -> &str {
        "list_dir"
    }

    fn description(&self) -> &str {
        "List files and subdirectories at the given path."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "Directory path to list." },
                "recursive": { "type": "boolean", "description": "Whether to list recursively." }
            },
            "required": ["path"]
        })
    }

    async fn execute(&self, _args: Value) -> Result<String> {
        Ok(String::new())
    }
}
