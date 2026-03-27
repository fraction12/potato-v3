//! Configuration schema — all user-configurable settings with defaults.

use serde::{Deserialize, Serialize};

use super::keybinds::KeybindConfig;

/// Top-level configuration for Potato.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Default agent to launch (e.g. `"claude"`, `"codex"`).
    pub default_agent: String,
    /// Path to the SQLite session database. Supports `~` expansion.
    pub db_path: String,
    /// UI refresh / tick rate in milliseconds.
    pub tick_rate_ms: u64,
    /// Keyboard bindings.
    pub keybinds: KeybindConfig,
    /// Active theme name (e.g. `"earth"`, `"nord"`).
    pub theme: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            default_agent: "claude".to_string(),
            db_path: "~/.potato/sessions.db".to_string(),
            tick_rate_ms: 250,
            keybinds: KeybindConfig::default(),
            theme: "earth".to_string(),
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config_values() {
        let cfg = Config::default();
        assert_eq!(cfg.default_agent, "claude");
        assert_eq!(cfg.db_path, "~/.potato/sessions.db");
        assert_eq!(cfg.theme, "earth");
        assert_eq!(cfg.tick_rate_ms, 250);
    }

    #[test]
    fn test_config_toml_deserialization() {
        let toml_str = r#"
            default_agent = "codex"
            theme = "nord"
        "#;
        let cfg: Config = toml::from_str(toml_str).expect("parse toml");
        assert_eq!(cfg.default_agent, "codex");
        assert_eq!(cfg.theme, "nord");
        // Fields not specified should use defaults.
        assert_eq!(cfg.db_path, "~/.potato/sessions.db");
    }
}
