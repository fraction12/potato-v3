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

/// Known valid key expression patterns.
fn is_valid_key_expr(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    let s = s.to_lowercase();
    // Single character
    if s.chars().count() == 1 {
        return true;
    }
    // Bare keys
    let bare = [
        "enter",
        "tab",
        "esc",
        "escape",
        "space",
        "backspace",
        "delete",
        "up",
        "down",
        "left",
        "right",
        "home",
        "end",
        "pageup",
        "pagedown",
    ];
    if bare.contains(&s.as_str()) {
        return true;
    }
    // Function keys: f1..f12
    if s.starts_with('f') {
        if let Ok(n) = s[1..].parse::<u8>() {
            return (1..=12).contains(&n);
        }
    }
    // Modifier combos: ctrl+x, alt+x, shift+x
    if let Some(rest) = s
        .strip_prefix("ctrl+")
        .or_else(|| s.strip_prefix("alt+"))
        .or_else(|| s.strip_prefix("shift+"))
    {
        return !rest.is_empty() && is_valid_key_expr(rest);
    }
    false
}

impl KeybindConfig {
    /// Return all bindings as (name, value) pairs for iteration.
    fn all_bindings(&self) -> Vec<(&str, &str)> {
        vec![
            ("quit", &self.quit),
            ("submit", &self.submit),
            ("model_picker", &self.model_picker),
            ("help", &self.help),
            ("approve", &self.approve),
            ("deny", &self.deny),
            ("new_session", &self.new_session),
            ("refresh", &self.refresh),
            ("focus_terminal", &self.focus_terminal),
        ]
    }

    /// Validate all keybind strings, logging warnings for invalid ones.
    /// Returns a list of warning messages (empty if all valid).
    pub fn validate(&self) -> Vec<String> {
        let mut warnings = Vec::new();
        for (name, value) in self.all_bindings() {
            if value.is_empty() {
                warnings.push(format!("keybind '{name}' is empty"));
            } else if !is_valid_key_expr(value) {
                warnings.push(format!(
                    "keybind '{name}': unrecognized key expression '{value}'"
                ));
            }
        }
        // Check for duplicate bindings.
        let values: Vec<&str> = self.all_bindings().iter().map(|(_, v)| *v).collect();
        for (i, (name_a, val_a)) in self.all_bindings().iter().enumerate() {
            for (name_b, val_b) in self.all_bindings().iter().skip(i + 1) {
                if val_a == val_b {
                    warnings.push(format!(
                        "keybind collision: '{name_a}' and '{name_b}' both map to '{val_a}'"
                    ));
                }
            }
        }
        let _ = values; // suppress unused
        warnings
    }
}

impl Default for KeybindConfig {
    fn default() -> Self {
        Self {
            quit: "ctrl+\\".to_string(),
            submit: "enter".to_string(),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_sensible() {
        let kb = KeybindConfig::default();
        assert_eq!(kb.quit, "ctrl+\\");
        assert_eq!(kb.submit, "enter");
        assert_eq!(kb.model_picker, "ctrl+m");
        assert_eq!(kb.help, "f1");
        assert_eq!(kb.approve, "y");
        assert_eq!(kb.deny, "n");
        assert_eq!(kb.new_session, "ctrl+n");
        assert_eq!(kb.refresh, "f5");
        assert_eq!(kb.focus_terminal, "f6");
    }

    #[test]
    fn serde_roundtrip_preserves_all_fields() {
        let kb = KeybindConfig::default();
        let toml_str = toml::to_string(&kb).expect("serialize");
        let decoded: KeybindConfig = toml::from_str(&toml_str).expect("deserialize");
        assert_eq!(decoded.quit, kb.quit);
        assert_eq!(decoded.submit, kb.submit);
        assert_eq!(decoded.model_picker, kb.model_picker);
        assert_eq!(decoded.help, kb.help);
        assert_eq!(decoded.approve, kb.approve);
        assert_eq!(decoded.deny, kb.deny);
        assert_eq!(decoded.new_session, kb.new_session);
        assert_eq!(decoded.refresh, kb.refresh);
        assert_eq!(decoded.focus_terminal, kb.focus_terminal);
    }

    #[test]
    fn partial_toml_fills_defaults() {
        let toml_str = r#"quit = "ctrl+q""#;
        let kb: KeybindConfig = toml::from_str(toml_str).expect("deserialize partial");
        assert_eq!(kb.quit, "ctrl+q", "explicit override should take effect");
        assert_eq!(kb.submit, "enter", "missing field should use default");
        assert_eq!(kb.help, "f1", "missing field should use default");
    }

    #[test]
    fn empty_toml_gives_all_defaults() {
        let kb: KeybindConfig = toml::from_str("").expect("deserialize empty");
        let defaults = KeybindConfig::default();
        assert_eq!(kb.quit, defaults.quit);
        assert_eq!(kb.submit, defaults.submit);
        assert_eq!(kb.approve, defaults.approve);
    }

    #[test]
    fn custom_overrides_serialize_correctly() {
        let kb = KeybindConfig {
            quit: "ctrl+q".to_string(),
            approve: "ctrl+y".to_string(),
            ..Default::default()
        };
        let toml_str = toml::to_string(&kb).expect("serialize");
        assert!(toml_str.contains(r#"quit = "ctrl+q""#));
        assert!(toml_str.contains(r#"approve = "ctrl+y""#));
        // Non-overridden fields still present
        assert!(toml_str.contains(r#"submit = "enter""#));
    }

    #[test]
    fn json_roundtrip() {
        let kb = KeybindConfig::default();
        let json = serde_json::to_string(&kb).expect("json serialize");
        let decoded: KeybindConfig = serde_json::from_str(&json).expect("json deserialize");
        assert_eq!(decoded.quit, kb.quit);
        assert_eq!(decoded.focus_terminal, kb.focus_terminal);
    }
}
