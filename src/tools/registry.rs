//! Tool registry — central store of all registered tools.

use std::{collections::HashMap, sync::Arc};

use super::Tool;

/// Holds all registered [`Tool`] implementations, keyed by name.
#[derive(Default)]
pub struct ToolRegistry {
    tools: HashMap<String, Arc<dyn Tool>>,
}

impl ToolRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a tool. Replaces any existing tool with the same name.
    pub fn register(&mut self, tool: Arc<dyn Tool>) {
        self.tools.insert(tool.name().to_string(), tool);
    }

    /// Look up a tool by name.
    pub fn get(&self, name: &str) -> Option<Arc<dyn Tool>> {
        self.tools.get(name).cloned()
    }

    /// Return names of all registered tools.
    pub fn tool_names(&self) -> Vec<&str> {
        self.tools.keys().map(|s| s.as_str()).collect()
    }

    /// Number of registered tools.
    pub fn len(&self) -> usize {
        self.tools.len()
    }

    /// Whether the registry has no tools.
    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::Tool;
    use anyhow::Result;
    use async_trait::async_trait;
    use serde_json::{Value, json};

    struct DummyTool { name: &'static str }

    #[async_trait]
    impl Tool for DummyTool {
        fn name(&self) -> &str { self.name }
        fn description(&self) -> &str { "dummy" }
        fn parameters_schema(&self) -> Value { json!({}) }
        async fn execute(&self, _args: Value) -> Result<String> { Ok(String::new()) }
    }

    #[test]
    fn test_register_and_get() {
        let mut reg = ToolRegistry::new();
        reg.register(Arc::new(DummyTool { name: "alpha" }));
        let got = reg.get("alpha");
        assert!(got.is_some());
        assert_eq!(got.unwrap().name(), "alpha");
    }

    #[test]
    fn test_get_nonexistent_returns_none() {
        let reg = ToolRegistry::new();
        assert!(reg.get("nonexistent").is_none());
    }

    #[test]
    fn test_tool_names() {
        let mut reg = ToolRegistry::new();
        reg.register(Arc::new(DummyTool { name: "one" }));
        reg.register(Arc::new(DummyTool { name: "two" }));
        let mut names = reg.tool_names();
        names.sort();
        assert_eq!(names, vec!["one", "two"]);
    }
}
