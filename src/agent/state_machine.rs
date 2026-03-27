//! Agent state machine — tracks which phase of the reasoning loop we are in.

/// The current execution phase of the AI agent.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum AgentState {
    /// Agent is idle, waiting for user input.
    #[default]
    Idle,
    /// Agent is generating a response (streaming tokens).
    Thinking,
    /// Agent has emitted a tool call and is waiting for execution.
    ToolCall {
        /// Name of the tool being invoked.
        tool_name: String,
    },
    /// Tool call requires user approval before execution.
    Approval {
        /// Name of the tool awaiting approval.
        tool_name: String,
        /// Serialised arguments.
        args: String,
    },
    /// Agent encountered an unrecoverable error.
    Error(String),
}
