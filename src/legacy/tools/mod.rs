//! Legacy tool system — retired from main code path; preserved for test coverage.

pub mod builtin;
pub mod executor;
pub mod registry;

use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;

/// All tool implementations must implement this trait.
#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn parameters_schema(&self) -> Value;
    async fn execute(&self, args: Value) -> Result<String>;
    fn requires_approval(&self) -> bool { false }
}
