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
        vec![AgentEvent::Raw {
            payload: line.to_string(),
        }]
    }

    fn format_user_input(&self, text: &str) -> String {
        format!("{text}\n")
    }

    fn format_approval(&self, _approved: bool) -> Option<String> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::AgentAdapter;

    #[test]
    fn name_returns_constructor_value() {
        let a = GenericAdapter::new("my-agent");
        assert_eq!(a.name(), "my-agent");
    }

    #[test]
    fn name_accepts_string_type() {
        let a = GenericAdapter::new(String::from("owned-name"));
        assert_eq!(a.name(), "owned-name");
    }

    #[test]
    fn capabilities_all_false() {
        let a = GenericAdapter::new("test");
        let caps = a.capabilities();
        assert!(!caps.structured_output);
        assert!(!caps.session_resumable);
        assert!(!caps.approval_intercept);
        assert!(!caps.tool_events);
    }

    #[test]
    fn detect_returns_none_for_nonexistent_binary() {
        let a = GenericAdapter::new("__potato_nonexistent_binary_xyz__");
        assert!(a.detect().is_none());
    }

    #[test]
    fn detect_returns_some_for_known_binary() {
        // `sh` should exist on every Unix system.
        let a = GenericAdapter::new("sh");
        assert!(a.detect().is_some());
    }

    #[test]
    fn parse_line_returns_single_raw_event() {
        let a = GenericAdapter::new("test");
        let events = a.parse_line("hello world");
        assert_eq!(events.len(), 1);
        match &events[0] {
            AgentEvent::Raw { payload } => assert_eq!(payload, "hello world"),
            other => panic!("expected Raw event, got {other:?}"),
        }
    }

    #[test]
    fn parse_line_preserves_empty_string() {
        let a = GenericAdapter::new("test");
        let events = a.parse_line("");
        assert_eq!(events.len(), 1);
        match &events[0] {
            AgentEvent::Raw { payload } => assert_eq!(payload, ""),
            other => panic!("expected Raw event, got {other:?}"),
        }
    }

    #[test]
    fn format_user_input_appends_newline() {
        let a = GenericAdapter::new("test");
        assert_eq!(a.format_user_input("hello"), "hello\n");
    }

    #[test]
    fn format_user_input_empty_string() {
        let a = GenericAdapter::new("test");
        assert_eq!(a.format_user_input(""), "\n");
    }

    #[test]
    fn format_approval_always_none() {
        let a = GenericAdapter::new("test");
        assert!(a.format_approval(true).is_none());
        assert!(a.format_approval(false).is_none());
    }

    #[test]
    fn build_command_uses_agent_name_and_working_dir() {
        let a = GenericAdapter::new("echo");
        let config = AdapterConfig {
            working_dir: std::path::PathBuf::from("/tmp"),
            model: None,
            resume_session_id: None,
            extra_flags: vec!["--flag".into(), "value".into()],
        };
        let cmd = a.build_command(&config);
        let as_std = cmd.as_std();
        assert_eq!(as_std.get_program(), "echo");
        let args: Vec<_> = as_std.get_args().collect();
        assert_eq!(args, &["--flag", "value"]);
        assert_eq!(as_std.get_current_dir(), Some(std::path::Path::new("/tmp")));
    }
}
