//! Legacy agent state machine — kept for UI compatibility.
//!
//! This is the old Ollama-era state machine. New code should use
//! [`crate::app::state::AgentStatus`] instead.

use anyhow::{bail, Result};
use serde_json::Value;

/// The current execution phase of the AI agent (legacy).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum AgentState {
    #[default]
    Idle,
    Thinking,
    ToolCall { tool_name: String },
    Approval { tool_name: String, args: String },
    Error(String),
}

impl AgentState {
    pub fn start_thinking(&self) -> Result<Self> {
        match self {
            Self::Idle | Self::ToolCall { .. } => Ok(Self::Thinking),
            other => bail!("cannot start_thinking from state {:?}", other),
        }
    }

    pub fn start_tool_call(&self, name: impl Into<String>) -> Result<Self> {
        match self {
            Self::Thinking => Ok(Self::ToolCall { tool_name: name.into() }),
            other => bail!("cannot start_tool_call from state {:?}", other),
        }
    }

    pub fn request_approval(&self, name: impl Into<String>, args: &Value) -> Result<Self> {
        match self {
            Self::Thinking => {
                let args_str = serde_json::to_string_pretty(args)
                    .unwrap_or_else(|_| args.to_string());
                Ok(Self::Approval { tool_name: name.into(), args: args_str })
            }
            other => bail!("cannot request_approval from state {:?}", other),
        }
    }

    pub fn complete(&self) -> Result<Self> {
        match self {
            Self::Error(e) => bail!("cannot complete from Error state: {e}"),
            _ => Ok(Self::Idle),
        }
    }

    pub fn fail(&self, error: impl Into<String>) -> Self {
        Self::Error(error.into())
    }

    pub fn is_active(&self) -> bool {
        matches!(self, Self::Thinking | Self::ToolCall { .. } | Self::Approval { .. })
    }
}
