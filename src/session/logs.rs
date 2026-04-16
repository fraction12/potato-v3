use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::claude_log::{ClaudeSessionLogTracker, ClaudeSidebarData, project_dir_name};
use crate::codex_log::{CodexSessionLogTracker, CodexSidebarData, find_session_log};

#[derive(Debug, Clone, Default)]
pub struct AgentLogSnapshot {
    pub model: Option<String>,
    pub title: String,
    pub turns: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
}

impl From<ClaudeSidebarData> for AgentLogSnapshot {
    fn from(value: ClaudeSidebarData) -> Self {
        let total_tokens = value.usage.total_tokens();
        Self {
            model: value.model,
            title: value.title,
            turns: value.turns,
            input_tokens: value.usage.input_tokens,
            output_tokens: value.usage.output_tokens,
            total_tokens,
        }
    }
}

impl From<CodexSidebarData> for AgentLogSnapshot {
    fn from(value: CodexSidebarData) -> Self {
        let total_tokens = value.usage.total_tokens();
        Self {
            model: value.model,
            title: value.title,
            turns: value.turns,
            input_tokens: value.usage.input_tokens,
            output_tokens: value.usage.output_tokens,
            total_tokens,
        }
    }
}

#[derive(Debug)]
pub enum AgentSessionLogTracker {
    Claude(ClaudeSessionLogTracker),
    Codex(CodexSessionLogTracker),
}

impl AgentSessionLogTracker {
    #[must_use]
    pub fn claude(path: PathBuf) -> Self {
        Self::Claude(ClaudeSessionLogTracker::new(path))
    }

    #[must_use]
    pub fn codex(path: PathBuf) -> Self {
        Self::Codex(CodexSessionLogTracker::new(path))
    }

    pub fn poll(&mut self) -> Result<bool> {
        match self {
            Self::Claude(tracker) => tracker.poll(),
            Self::Codex(tracker) => tracker.poll(),
        }
    }

    #[must_use]
    pub fn snapshot(&self) -> AgentLogSnapshot {
        match self {
            Self::Claude(tracker) => tracker.snapshot().into(),
            Self::Codex(tracker) => tracker.snapshot().into(),
        }
    }
}

#[must_use]
pub fn provider_project_dir_name(cwd: &Path) -> String {
    project_dir_name(cwd)
}

#[must_use]
pub fn codex_session_log_path(home: &Path, session_id: &str) -> Option<PathBuf> {
    find_session_log(home, session_id)
}

#[must_use]
pub fn session_log_path_for(
    home: &Path,
    cwd: &Path,
    agent: &str,
    session_id: &str,
) -> Option<PathBuf> {
    match agent {
        "claude" => Some(
            home.join(".claude")
                .join("projects")
                .join(provider_project_dir_name(cwd))
                .join(format!("{session_id}.jsonl")),
        ),
        "codex" => codex_session_log_path(home, session_id),
        _ => None,
    }
}
