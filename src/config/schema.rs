//! Configuration schema — all user-configurable settings with defaults.

use serde::{Deserialize, Serialize};

use super::keybinds::KeybindConfig;

/// Top-level configuration for Potato.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Model to use by default (e.g. `"llama3"`, `"gpt-4o"`).
    pub model: String,
    /// Base URL for the local Ollama instance.
    pub ollama_url: String,
    /// Cloud provider API key (OpenAI-compatible endpoints).
    pub api_key: Option<String>,
    /// Cloud provider base URL override (e.g. for Azure, Together, etc.).
    pub api_base_url: Option<String>,
    /// Path to the SQLite session database. Supports `~` expansion.
    pub db_path: String,
    /// Whether to require user approval for every tool call.
    pub require_approval: bool,
    /// Maximum messages to retain in history (0 = unlimited).
    pub max_history: usize,
    /// UI refresh / tick rate in milliseconds.
    pub tick_rate_ms: u64,
    /// Keyboard bindings.
    pub keybinds: KeybindConfig,
    /// Active theme name (e.g. `"earth"`, `"nord"`).
    pub theme: String,
    /// Default tool execution timeout in seconds.
    pub tool_timeout_secs: u64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            model: "llama3".to_string(),
            ollama_url: "http://localhost:11434".to_string(),
            api_key: None,
            api_base_url: None,
            db_path: "~/.potato/sessions.db".to_string(),
            require_approval: true,
            max_history: 0,
            tick_rate_ms: 250,
            keybinds: KeybindConfig::default(),
            theme: "earth".to_string(),
            tool_timeout_secs: 30,
        }
    }
}
