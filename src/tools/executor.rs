//! Tool executor — looks up and runs tools from the registry.

use anyhow::{bail, Result};
use serde_json::Value;

use super::registry::ToolRegistry;

/// Execute the named tool with the given JSON arguments.
///
/// Returns the tool's string output, or an error if the tool is not found
/// or execution fails.
pub async fn execute_tool(
    registry: &ToolRegistry,
    tool_name: &str,
    args: Value,
) -> Result<String> {
    let tool = registry
        .get(tool_name)
        .ok_or_else(|| anyhow::anyhow!("Tool '{}' not found in registry", tool_name))?;

    tool.execute(args).await
}

/// Execute the named tool; on failure, return a formatted error string
/// instead of propagating the error (safe variant for agent loops).
pub async fn execute_tool_safe(
    registry: &ToolRegistry,
    tool_name: &str,
    args: Value,
) -> String {
    match execute_tool(registry, tool_name, args).await {
        Ok(output) => output,
        Err(e) => format!("[tool error: {}]", e),
    }
}

/// Validate tool name is registered without executing it.
pub fn validate_tool(registry: &ToolRegistry, tool_name: &str) -> Result<()> {
    if registry.get(tool_name).is_some() {
        Ok(())
    } else {
        bail!("Tool '{}' is not registered", tool_name)
    }
}
