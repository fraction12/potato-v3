//! Application-level messages that drive the update loop.

use crossterm::event::{KeyEvent, MouseEvent};

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

/// Events produced by the agent loop.
#[derive(Debug, Clone)]
pub enum AgentEvent {
    /// A new text token was streamed.
    Token(String),
    /// The agent completed a response.
    ResponseComplete,
    /// The agent is requesting tool approval.
    ApprovalRequired { tool_name: String, args: String },
    /// A tool finished executing.
    ToolComplete { tool_name: String, output: String },
    /// The agent encountered an error.
    Error(String),
}
