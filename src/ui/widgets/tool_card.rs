//! Tool card widget — compact display of a tool call and its result.

/// Displays a tool invocation (name, args) and optionally its output.
#[derive(Debug, Default)]
pub struct ToolCard {
    /// Name of the tool that was called.
    pub tool_name: String,
    /// JSON-formatted arguments.
    pub args: String,
    /// Optional output from the tool.
    pub output: Option<String>,
    /// Whether the tool call is still in progress.
    pub pending: bool,
}

impl ToolCard {
    /// Create a new pending [`ToolCard`].
    pub fn new(tool_name: impl Into<String>, args: impl Into<String>) -> Self {
        Self {
            tool_name: tool_name.into(),
            args: args.into(),
            output: None,
            pending: true,
        }
    }
}
