//! Configuration schema — all user-configurable settings with defaults.

use serde::{Deserialize, Serialize};

use super::keybinds::KeybindConfig;

/// Top-level configuration for Potato.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Model to use by default.
    pub model: String,
    /// Base URL for the Ollama instance.
    pub ollama_url: String,
    /// Path to the SQLite session database.
    pub db_path: String,
    /// Whether to require approval for every tool call.
    pub require_approval: bool,
    /// Maximum messages to retain in history (0 = unlimited).
    pub max_history: usize,
    /// Tick rate in milliseconds.
    pub tick_rate_ms: u64,
    /// Keyboard bindings.
    pub keybinds: KeybindConfig,
    /// Active theme name.
    pub theme: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            model: "llama3".to_string(),
            ollama_url: "http://localhost:11434".to_string(),
            db_path: "~/.potato/sessions.db".to_string(),
            require_approval: true,
            max_history: 0,
            tick_rate_ms: 250,
            keybinds: KeybindConfig::default(),
            theme: "earth".to_string(),
        }
    }
}
