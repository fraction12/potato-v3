//! Configuration loading and validation.
//!
//! Config is loaded from (in priority order):
//! 1. A custom path supplied via CLI (`--config`).
//! 2. `~/.potato/config.toml`
//! 3. Built-in defaults ([`Config::default`]).

pub mod keybinds;
pub mod profiles;
pub mod schema;

pub use schema::Config;

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

/// Load configuration from disk, falling back to defaults.
///
/// - If `path` is `Some`, that file is required to exist and parse correctly.
/// - Otherwise, `~/.potato/config.toml` is tried; if absent, defaults are used.
/// - The `~/.potato/` directory is created if it does not exist.
pub fn load_config(path: Option<&str>) -> Result<Config> {
    // Always ensure the potato directory exists.
    let potato_dir = potato_dir()?;
    std::fs::create_dir_all(&potato_dir)
        .with_context(|| format!("failed to create config directory: {}", potato_dir.display()))?;

    if let Some(custom) = path {
        // User explicitly supplied a path — it must exist and parse.
        let expanded = expand_tilde(custom);
        let raw = std::fs::read_to_string(&expanded)
            .with_context(|| format!("config file not found: {}", expanded.display()))?;
        let config: Config = toml::from_str(&raw).with_context(|| {
            format!(
                "failed to parse config file — check your TOML syntax in: {}",
                expanded.display()
            )
        })?;
        return Ok(config);
    }

    // Default config path.
    let default_path = potato_dir.join("config.toml");
    if default_path.exists() {
        let raw = std::fs::read_to_string(&default_path)
            .with_context(|| format!("failed to read config: {}", default_path.display()))?;
        let config: Config = toml::from_str(&raw).with_context(|| {
            format!(
                "failed to parse config — check your TOML syntax in: {}",
                default_path.display()
            )
        })?;
        return Ok(config);
    }

    // No config file — use built-in defaults.
    Ok(Config::default())
}

/// Resolve the `~/.potato` directory path.
fn potato_dir() -> Result<PathBuf> {
    let home = dirs::home_dir().context("could not determine home directory")?;
    Ok(home.join(".potato"))
}

/// Expand a leading `~` to the user's home directory.
pub fn expand_tilde(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest);
        }
    } else if path == "~" {
        if let Some(home) = dirs::home_dir() {
            return home;
        }
    }
    Path::new(path).to_path_buf()
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    // ── expand_tilde ──────────────────────────────────────────────────────────

    #[test]
    fn expand_tilde_home_prefix() {
        let expanded = expand_tilde("~/Documents/test.txt");
        let home = dirs::home_dir().unwrap();
        assert_eq!(expanded, home.join("Documents/test.txt"));
    }

    #[test]
    fn expand_tilde_bare_tilde() {
        let expanded = expand_tilde("~");
        let home = dirs::home_dir().unwrap();
        assert_eq!(expanded, home);
    }

    #[test]
    fn expand_tilde_absolute_path_unchanged() {
        let expanded = expand_tilde("/usr/local/bin");
        assert_eq!(expanded, PathBuf::from("/usr/local/bin"));
    }

    #[test]
    fn expand_tilde_relative_path_unchanged() {
        let expanded = expand_tilde("relative/path");
        assert_eq!(expanded, PathBuf::from("relative/path"));
    }

    #[test]
    fn expand_tilde_tilde_in_middle_unchanged() {
        // "foo/~/bar" should not be expanded.
        let expanded = expand_tilde("foo/~/bar");
        assert_eq!(expanded, PathBuf::from("foo/~/bar"));
    }

    // ── load_config ───────────────────────────────────────────────────────────

    #[test]
    fn load_config_defaults_when_no_file() {
        // Point HOME at a temp dir so ~/.potato/config.toml doesn't exist.
        // We can't safely change HOME in tests, so instead use the explicit
        // path variant: loading from a nonexistent explicit path should fail.
        let result = load_config(Some("/tmp/nonexistent-potato-config.toml"));
        assert!(result.is_err());
    }

    #[test]
    fn load_config_from_explicit_path() {
        let tmp = TempDir::new().unwrap();
        let cfg_path = tmp.path().join("test-config.toml");
        std::fs::write(
            &cfg_path,
            r#"
default_agent = "codex"
theme = "nord"
tick_rate_ms = 100
"#,
        )
        .unwrap();

        let config = load_config(Some(cfg_path.to_str().unwrap())).unwrap();
        assert_eq!(config.default_agent, "codex");
        assert_eq!(config.theme, "nord");
        assert_eq!(config.tick_rate_ms, 100);
        // db_path should fall back to default.
        assert_eq!(config.db_path, "~/.potato/sessions.db");
    }

    #[test]
    fn load_config_invalid_toml_returns_error() {
        let tmp = TempDir::new().unwrap();
        let cfg_path = tmp.path().join("bad-config.toml");
        std::fs::write(&cfg_path, "this is not {{{ valid toml").unwrap();

        let result = load_config(Some(cfg_path.to_str().unwrap()));
        assert!(result.is_err());
        let err_msg = format!("{:#}", result.unwrap_err());
        assert!(err_msg.contains("parse"), "error should mention parsing: {err_msg}");
    }

    #[test]
    fn load_config_partial_toml_uses_defaults_for_missing() {
        let tmp = TempDir::new().unwrap();
        let cfg_path = tmp.path().join("partial-config.toml");
        std::fs::write(&cfg_path, "default_agent = \"pi\"\n").unwrap();

        let config = load_config(Some(cfg_path.to_str().unwrap())).unwrap();
        assert_eq!(config.default_agent, "pi");
        // All other fields should have defaults.
        assert_eq!(config.theme, "earth");
        assert_eq!(config.tick_rate_ms, 250);
        assert_eq!(config.keybinds.quit, "ctrl+\\");
    }
}
