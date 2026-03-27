//! Agent state machine — tracks which phase of the reasoning loop we are in.

use anyhow::{bail, Result};
use serde_json::Value;

/// The current execution phase of the AI agent.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum AgentState {
    /// Agent is idle, waiting for user input.
    #[default]
    Idle,
    /// Agent is generating a response (streaming tokens from the LLM).
    Thinking,
    /// Agent has emitted a tool call and is waiting for execution to complete.
    ToolCall {
        /// Name of the tool being invoked.
        tool_name: String,
    },
    /// Tool call requires user approval before execution.
    Approval {
        /// Name of the tool awaiting approval.
        tool_name: String,
        /// Serialised arguments (JSON string for display).
        args: String,
    },
    /// Agent encountered an unrecoverable error.
    Error(String),
}

impl AgentState {
    /// Transition from [`AgentState::Idle`] → [`AgentState::Thinking`].
    ///
    /// Returns `Err` if the current state does not permit this transition.
    pub fn start_thinking(&self) -> Result<Self> {
        match self {
            Self::Idle | Self::ToolCall { .. } => Ok(Self::Thinking),
            other => bail!("cannot start_thinking from state {:?}", other),
        }
    }

    /// Transition from [`AgentState::Thinking`] → [`AgentState::ToolCall`].
    ///
    /// Returns `Err` if the current state does not permit this transition.
    pub fn start_tool_call(&self, name: impl Into<String>) -> Result<Self> {
        match self {
            Self::Thinking => Ok(Self::ToolCall { tool_name: name.into() }),
            other => bail!("cannot start_tool_call from state {:?}", other),
        }
    }

    /// Transition from [`AgentState::Thinking`] → [`AgentState::Approval`].
    ///
    /// `args` is a [`serde_json::Value`] that will be serialised for display.
    ///
    /// Returns `Err` if the current state does not permit this transition.
    pub fn request_approval(&self, name: impl Into<String>, args: &Value) -> Result<Self> {
        match self {
            Self::Thinking => {
                let args_str = serde_json::to_string_pretty(args)
                    .unwrap_or_else(|_| args.to_string());
                Ok(Self::Approval {
                    tool_name: name.into(),
                    args: args_str,
                })
            }
            other => bail!("cannot request_approval from state {:?}", other),
        }
    }

    /// Transition any non-error state → [`AgentState::Idle`].
    ///
    /// Calling `complete()` from [`AgentState::Error`] returns `Err` — errors
    /// must be explicitly cleared by the caller.
    pub fn complete(&self) -> Result<Self> {
        match self {
            Self::Error(e) => bail!("cannot complete from Error state: {e}"),
            _ => Ok(Self::Idle),
        }
    }

    /// Transition any state → [`AgentState::Error`].
    pub fn fail(&self, error: impl Into<String>) -> Self {
        Self::Error(error.into())
    }

    /// Return `true` if the agent is currently active (not idle or errored).
    pub fn is_active(&self) -> bool {
        matches!(self, Self::Thinking | Self::ToolCall { .. } | Self::Approval { .. })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn happy_path_transitions() {
        let s = AgentState::Idle;
        let s = s.start_thinking().unwrap();
        assert_eq!(s, AgentState::Thinking);

        let s = s
            .request_approval("shell", &serde_json::json!({"cmd": "ls"}))
            .unwrap();
        assert!(matches!(s, AgentState::Approval { .. }));

        let s = s.complete().unwrap();
        assert_eq!(s, AgentState::Idle);
    }

    #[test]
    fn invalid_transition_returns_err() {
        let s = AgentState::Idle;
        assert!(s.start_tool_call("foo").is_err());
    }

    #[test]
    fn fail_from_any_state() {
        let s = AgentState::Thinking;
        let s = s.fail("network error");
        assert_eq!(s, AgentState::Error("network error".into()));
    }
}
