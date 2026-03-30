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
    /// Key to refresh git/tasks/status.
    pub refresh: String,
    /// Key to jump to terminal focus.
    pub focus_terminal: String,
}

impl Default for KeybindConfig {
    fn default() -> Self {
        Self {
            quit: "ctrl+\\".to_string(),
            submit: "enter".to_string(),
            slash_menu: "/".to_string(),
            model_picker: "ctrl+m".to_string(),
            help: "f1".to_string(),
            approve: "y".to_string(),
            deny: "n".to_string(),
            new_session: "ctrl+n".to_string(),
            refresh: "f5".to_string(),
            focus_terminal: "f6".to_string(),
        }
    }
}
