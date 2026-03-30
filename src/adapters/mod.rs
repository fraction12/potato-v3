//! Agent adapter system — each adapter wraps a specific coding agent CLI.
//!
//! Adapters translate between the agent's wire format and Potato's canonical
//! [`AgentEvent`] stream.  They also handle process construction and I/O formatting.

pub mod claude;
pub mod codex;
pub mod generic;

use std::path::PathBuf;

use tokio::process::Command;

use crate::events::AgentEvent;

// ── AdapterCapabilities ───────────────────────────────────────────────────────

/// Feature flags for a specific adapter.
#[derive(Debug, Clone)]
pub struct AdapterCapabilities {
    /// Whether the adapter emits structured (parsed) output vs raw text.
    pub structured_output: bool,
    /// Whether the adapter supports resuming a prior session by id.
    pub session_resumable: bool,
    /// Whether the adapter can intercept tool approval requests.
    pub approval_intercept: bool,
    /// Whether the adapter emits discrete tool start/done events.
    pub tool_events: bool,
}

// ── AdapterConfig ─────────────────────────────────────────────────────────────

/// Runtime configuration passed to an adapter when spawning a process.
#[derive(Debug, Clone)]
pub struct AdapterConfig {
    /// Directory the agent should operate in.
    pub working_dir: PathBuf,
    /// Optional model override (e.g. `"claude-opus-4-5"`).
    pub model: Option<String>,
    /// Optional session id for resumable agents.
    pub resume_session_id: Option<String>,
    /// Extra flags appended verbatim to the CLI invocation.
    pub extra_flags: Vec<String>,
}

impl Default for AdapterConfig {
    fn default() -> Self {
        Self {
            working_dir: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            model: None,
            resume_session_id: None,
            extra_flags: Vec::new(),
        }
    }
}

// ── AgentAdapter trait ────────────────────────────────────────────────────────

/// Interface implemented by every agent adapter.
///
/// Adapters are stateless wrappers — they do not hold process handles or
/// channels; that is the responsibility of [`crate::pty::PtyProcess`].
pub trait AgentAdapter: Send + Sync {
    /// Short identifier for this adapter (e.g. `"claude"`).
    fn name(&self) -> &str;

    /// Attempt to locate the agent binary on the current machine.
    ///
    /// Returns the full path if found, or `None` if the binary is not available.
    fn detect(&self) -> Option<PathBuf>;

    /// Describe what this adapter can do.
    fn capabilities(&self) -> AdapterCapabilities;

    /// Build the [`Command`] that will spawn the agent process.
    fn build_command(&self, config: &AdapterConfig) -> Command;

    /// Parse a single line of agent output into zero or more canonical events.
    fn parse_line(&self, line: &str) -> Vec<AgentEvent>;

    /// Format user input text for transmission to the agent's stdin.
    fn format_user_input(&self, text: &str) -> String;

    /// Format an approval decision for transmission to the agent's stdin.
    ///
    /// Returns `None` if the adapter does not support approval intercept.
    fn format_approval(&self, approved: bool) -> Option<String>;
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::claude::ClaudeAdapter;
    use crate::adapters::codex::CodexAdapter;
    use crate::adapters::generic::GenericAdapter;

    #[test]
    fn generic_adapter_name() {
        let a = GenericAdapter::new("myagent");
        assert_eq!(a.name(), "myagent");
    }

    #[test]
    fn generic_adapter_capabilities_are_minimal() {
        let a = GenericAdapter::new("x");
        let caps = a.capabilities();
        assert!(!caps.structured_output);
        assert!(!caps.tool_events);
    }

    #[test]
    fn generic_adapter_parse_line_is_raw() {
        let a = GenericAdapter::new("x");
        let events = a.parse_line("some output");
        assert_eq!(events.len(), 1);
        assert!(matches!(&events[0], AgentEvent::Raw { payload } if payload == "some output"));
    }

    #[test]
    fn generic_adapter_format_user_input() {
        let a = GenericAdapter::new("x");
        assert_eq!(a.format_user_input("hello"), "hello\n");
    }

    #[test]
    fn generic_adapter_format_approval_none() {
        let a = GenericAdapter::new("x");
        assert!(a.format_approval(true).is_none());
    }

    #[test]
    fn claude_adapter_name() {
        let a = ClaudeAdapter;
        assert_eq!(a.name(), "claude");
    }

    #[test]
    fn claude_adapter_capabilities() {
        let a = ClaudeAdapter;
        let caps = a.capabilities();
        assert!(caps.structured_output);
        assert!(caps.approval_intercept);
        assert!(caps.tool_events);
    }

    #[test]
    fn claude_adapter_format_user_input() {
        let a = ClaudeAdapter;
        assert_eq!(a.format_user_input("hi"), "hi\n");
    }

    #[test]
    fn claude_adapter_format_approval_yes() {
        let a = ClaudeAdapter;
        assert_eq!(a.format_approval(true), Some("y\n".to_string()));
    }

    #[test]
    fn claude_adapter_format_approval_no() {
        let a = ClaudeAdapter;
        assert_eq!(a.format_approval(false), Some("n\n".to_string()));
    }

    #[test]
    fn codex_adapter_name() {
        let a = CodexAdapter;
        assert_eq!(a.name(), "codex");
    }

    #[test]
    fn codex_adapter_capabilities_structured_and_resumable() {
        let a = CodexAdapter;
        let caps = a.capabilities();
        assert!(caps.structured_output);
        assert!(caps.session_resumable);
        assert!(!caps.approval_intercept);
        assert!(caps.tool_events);
    }

    #[test]
    fn generic_adapter_build_command_sets_working_dir() {
        let a = GenericAdapter::new("myagent");
        let config = AdapterConfig {
            working_dir: std::path::PathBuf::from("/var/test"),
            model: None,
            resume_session_id: None,
            extra_flags: vec![],
        };
        let cmd = a.build_command(&config);
        assert_eq!(
            cmd.as_std().get_current_dir(),
            Some(std::path::Path::new("/var/test")),
            "GenericAdapter build_command must set working dir"
        );
    }

    #[test]
    fn claude_adapter_parse_raw_line() {
        let a = ClaudeAdapter;
        let events = a.parse_line("not json");
        assert!(matches!(&events[0], AgentEvent::Raw { .. }));
    }

    #[test]
    fn claude_adapter_parse_session_init() {
        let a = ClaudeAdapter;
        let line = r#"{"type":"system","subtype":"init","session_id":"abc-123","tools":[]}"#;
        let events = a.parse_line(line);
        assert_eq!(events.len(), 1);
        assert!(
            matches!(&events[0], AgentEvent::SessionBound { agent_session_id } if agent_session_id == "abc-123")
        );
    }
}
