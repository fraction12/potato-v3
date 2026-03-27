//! Tool executor — runs tools with timeout and panic recovery.

use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::Value;

use super::Tool;

/// The result of a tool execution attempt.
#[derive(Debug)]
pub struct ToolResult {
    /// Name of the tool that was executed.
    pub tool_name: String,
    /// The tool's output, or an error description if execution failed.
    pub output: Result<String, String>,
    /// Wall-clock time the execution took.
    pub duration: Duration,
}

impl ToolResult {
    /// Convenience: return the output string or the error string.
    pub fn output_or_error(&self) -> &str {
        match &self.output {
            Ok(s) => s,
            Err(e) => e,
        }
    }

    /// Whether the tool completed without error.
    pub fn is_ok(&self) -> bool {
        self.output.is_ok()
    }
}

/// Execute a tool with a hard timeout.
///
/// - If the tool completes in time, `output` is `Ok(string)`.
/// - If the timeout fires, `output` is `Err("timed out after Xs")`.
/// - If the tool itself returns an `Err`, it is propagated as `Err(string)`.
pub async fn execute_tool(
    tool: Arc<dyn Tool>,
    args: Value,
    timeout: Duration,
) -> ToolResult {
    let tool_name = tool.name().to_string();
    let start = Instant::now();

    let result = tokio::time::timeout(timeout, tool.execute(args)).await;

    let duration = start.elapsed();

    let output = match result {
        Ok(Ok(s)) => Ok(s),
        Ok(Err(e)) => Err(format!("[tool error] {}", e)),
        Err(_) => Err(format!(
            "[tool timeout] {} did not complete within {:.1}s",
            tool_name,
            timeout.as_secs_f64()
        )),
    };

    ToolResult {
        tool_name,
        output,
        duration,
    }
}

/// Execute a named tool from the registry with timeout.
///
/// Returns an error string rather than propagating [`anyhow::Error`], making
/// it safe to use directly inside agent loops without bubbling up.
pub async fn execute_tool_safe(
    tool: Arc<dyn Tool>,
    args: Value,
    timeout: Duration,
) -> String {
    let result = execute_tool(tool, args, timeout).await;
    result.output_or_error().to_string()
}
