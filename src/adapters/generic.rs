//! Generic adapter — passes all agent output through as [`AgentEvent::Raw`].
//!
//! Use this as a fallback for any agent that does not have a dedicated adapter.

use std::path::PathBuf;

use tokio::process::Command;

use super::{AdapterCapabilities, AdapterConfig, AgentAdapter};
use crate::events::AgentEvent;

/// A pass-through adapter that treats every output line as a raw event.
pub struct GenericAdapter {
    name: String,
}

impl GenericAdapter {
    /// Create a generic adapter for an agent with the given name.
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into() }
    }
}

impl AgentAdapter for GenericAdapter {
    fn name(&self) -> &str {
        &self.name
    }

    fn detect(&self) -> Option<PathBuf> {
        which::which(&self.name).ok()
    }

    fn capabilities(&self) -> AdapterCapabilities {
        AdapterCapabilities {
            structured_output: false,
            session_resumable: false,
            approval_intercept: false,
            tool_events: false,
        }
    }

    fn build_command(&self, config: &AdapterConfig) -> Command {
        let mut cmd = Command::new(&self.name);
        cmd.current_dir(&config.working_dir);
        for flag in &config.extra_flags {
            cmd.arg(flag);
        }
        cmd
    }

    fn parse_line(&self, line: &str) -> Vec<AgentEvent> {
        vec![AgentEvent::Raw { payload: line.to_string() }]
    }

    fn format_user_input(&self, text: &str) -> String {
        format!("{text}\n")
    }

    fn format_approval(&self, _approved: bool) -> Option<String> {
        None
    }
}
