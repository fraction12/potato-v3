//! Approval bar widget — inline prompt for approving/denying a tool call.

/// Bottom-of-panel bar that asks the user to approve or deny a tool execution.
#[derive(Debug, Default)]
pub struct ApprovalBar {
    /// Tool name awaiting approval.
    pub tool_name: String,
    /// Serialised arguments for the pending tool call.
    pub args: String,
}

impl ApprovalBar {
    /// Create a new [`ApprovalBar`] for the given tool.
    pub fn new(tool_name: impl Into<String>, args: impl Into<String>) -> Self {
        Self {
            tool_name: tool_name.into(),
            args: args.into(),
        }
    }
}
