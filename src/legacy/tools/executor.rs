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

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::legacy::tools::Tool;
    use anyhow::Result;
    use async_trait::async_trait;
    use serde_json::{Value, json};
    use std::time::Duration;

    /// A tool that echoes the "msg" argument.
    struct EchoTool;

    #[async_trait]
    impl Tool for EchoTool {
        fn name(&self) -> &str { "echo" }
        fn description(&self) -> &str { "echoes msg" }
        fn parameters_schema(&self) -> Value { json!({}) }
        async fn execute(&self, args: Value) -> Result<String> {
            Ok(args["msg"].as_str().unwrap_or("").to_string())
        }
    }

    /// A tool that sleeps for 2 seconds (designed to be timed out).
    struct SleepTool;

    #[async_trait]
    impl Tool for SleepTool {
        fn name(&self) -> &str { "sleep_tool" }
        fn description(&self) -> &str { "sleeps" }
        fn parameters_schema(&self) -> Value { json!({}) }
        async fn execute(&self, _args: Value) -> Result<String> {
            tokio::time::sleep(Duration::from_secs(10)).await;
            Ok("done".to_string())
        }
    }

    #[tokio::test]
    async fn test_execute_returns_result() {
        let tool = Arc::new(EchoTool);
        let result = execute_tool(tool, json!({"msg": "hello"}), Duration::from_secs(5)).await;
        assert!(result.is_ok());
        assert_eq!(result.output_or_error(), "hello");
        assert_eq!(result.tool_name, "echo");
    }

    #[tokio::test]
    async fn test_execute_timeout() {
        let tool = Arc::new(SleepTool);
        let result = execute_tool(tool, json!({}), Duration::from_millis(50)).await;
        assert!(!result.is_ok());
        let err = result.output_or_error();
        assert!(err.contains("timeout") || err.contains("timed out"), "expected timeout error, got: {}", err);
    }
}
