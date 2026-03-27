//! Search tool — grep-style pattern search across files.

use anyhow::Result;
use async_trait::async_trait;
use serde_json::{Value, json};

use crate::tools::Tool;

/// Searches for a regex or literal pattern across a directory of files.
#[derive(Debug, Default)]
pub struct SearchTool;

#[async_trait]
impl Tool for SearchTool {
    fn name(&self) -> &str {
        "search"
    }

    fn description(&self) -> &str {
        "Search for a pattern in files (grep-style)."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "pattern": { "type": "string", "description": "Regex or literal search pattern." },
                "path":    { "type": "string", "description": "Directory or file to search." },
                "literal": { "type": "boolean", "description": "If true, treat pattern as literal string." }
            },
            "required": ["pattern"]
        })
    }

    async fn execute(&self, _args: Value) -> Result<String> {
        Ok(String::new())
    }
}
