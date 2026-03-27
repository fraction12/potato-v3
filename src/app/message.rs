//! Application-level messages that drive the update loop.

use crossterm::event::{KeyEvent, MouseEvent};
use serde_json::Value;

/// All messages that can be sent to the application update function.
#[derive(Debug, Clone)]
pub enum Message {
    /// Periodic timer tick (drives animations, background polling).
    Tick,
    /// A keyboard event from the terminal.
    Key(KeyEvent),
    /// A mouse event from the terminal.
    Mouse(MouseEvent),
    /// The terminal was resized to (cols, rows).
    Resize(u16, u16),
    /// A message or event originating from the agent loop.
    Agent(AgentEvent),
    /// Request to quit the application.
    Quit,
}

/// Events produced by the agent loop and consumed by the UI.
#[derive(Debug, Clone)]
pub enum AgentEvent {
    /// A new text token was streamed from the LLM.
    Token(String),
    /// The agent completed a full response turn.
    ResponseComplete,
    /// The agent is requesting approval before invoking a tool.
    ApprovalRequired {
        /// Name of the tool the agent wants to call.
        tool_name: String,
        /// The tool arguments as a JSON string (pretty-printed for display).
        args: String,
    },
    /// The agent emitted a tool call that is about to be executed (or queued for approval).
    ToolCallRequested {
        /// Name of the tool.
        tool_name: String,
        /// Full argument payload.
        args: Value,
    },
    /// A tool finished executing.
    ToolComplete {
        /// Name of the tool that ran.
        tool_name: String,
        /// Stdout / result output from the tool.
        output: String,
    },
    /// The agent encountered an unrecoverable error.
    Error(String),
}

/// Commands sent **into** the agent loop from the UI (approval responses, new input, etc.).
#[derive(Debug)]
pub enum AgentCommand {
    /// Approve (`true`) or deny (`false`) a pending tool call.
    Approve(bool),
    /// A new user message to send to the LLM.
    UserMessage(String),
    /// Cancel the current in-flight request.
    Cancel,
}
