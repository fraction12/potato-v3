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
