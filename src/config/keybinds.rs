//! Keyboard binding configuration.

use serde::{Deserialize, Serialize};

/// User-configurable keyboard shortcuts.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct KeybindConfig {
    /// Key to quit the application.
    pub quit: String,
    /// Key to submit the current input.
    pub submit: String,
    /// Key to open the slash-command menu.
    pub slash_menu: String,
    /// Key to open the model picker.
    pub model_picker: String,
    /// Key to open the help overlay.
    pub help: String,
    /// Key to approve a pending tool call.
    pub approve: String,
    /// Key to deny a pending tool call.
    pub deny: String,
    /// Key to create a new session.
    pub new_session: String,
}

impl Default for KeybindConfig {
    fn default() -> Self {
        Self {
            quit: "ctrl+q".to_string(),
            submit: "enter".to_string(),
            slash_menu: "/".to_string(),
            model_picker: "ctrl+m".to_string(),
            help: "?".to_string(),
            approve: "y".to_string(),
            deny: "n".to_string(),
            new_session: "ctrl+n".to_string(),
        }
    }
}
