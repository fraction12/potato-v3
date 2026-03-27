//! Tool system — trait, registry, executor, and built-in implementations.

pub mod builtin;
pub mod executor;
pub mod registry;

use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;

/// All tool implementations must implement this trait.
#[async_trait]
pub trait Tool: Send + Sync {
    /// Unique slug name for this tool (e.g. `"shell"`, `"read_file"`).
    fn name(&self) -> &str;

    /// Human-readable description shown to the LLM and in the help overlay.
    fn description(&self) -> &str;

    /// JSON Schema describing the tool's input parameters.
    fn parameters_schema(&self) -> Value;

    /// Execute the tool with the given JSON arguments.
    async fn execute(&self, args: Value) -> Result<String>;

    /// Whether this tool requires user approval before execution.
    fn requires_approval(&self) -> bool {
        false
    }
}
