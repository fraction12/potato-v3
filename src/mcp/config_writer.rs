//! Dynamic `.mcp.json` lifecycle management for Potato.
//!
//! Writes per-pane MCP server entries into `.mcp.json` in the project directory,
//! merging with any existing user-defined MCP servers. Only touches `potato-*` keys.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde_json::{json, Map, Value};

// ── Public API ────────────────────────────────────────────────────────────────

/// Write (or merge) Potato's single MCP server entry into `<project_dir>/.mcp.json`.
///
/// Creates ONE shared `"potato"` entry. Each Claude session spawns its own
/// `potato mcp-server` process (stdio is inherently 1:1). The process inherits
/// `POTATO_PANE_ID` and `POTATO_SOCKET` from its parent Claude PTY process,
/// so no per-pane config entries are needed.
///
/// Also cleans up any legacy `potato-*` per-pane entries from older versions.
///
/// Existing entries that are NOT `potato` or `potato-*` are preserved unchanged.
pub fn write_mcp_config(
    project_dir: &Path,
    _pane_ids: &[u64],
    _socket_path: &str,
) -> std::io::Result<()> {
    let config_path = mcp_config_path(project_dir);

    // Load existing config (or start with empty object).
    let mut config = load_config(&config_path).unwrap_or_else(|| json!({}));

    // Ensure mcpServers key exists.
    let obj = config.as_object_mut().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "existing .mcp.json is not a JSON object",
        )
    })?;
    let servers = obj
        .entry("mcpServers")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "mcpServers in .mcp.json is not a JSON object",
            )
        })?;

    // Remove legacy per-pane entries (potato-0, potato-1, etc.).
    servers.retain(|k, _| !k.starts_with("potato-"));

    // Write the single shared entry. POTATO_PANE_ID and POTATO_SOCKET
    // are inherited from the Claude PTY process environment.
    servers.insert("potato".into(), potato_server_entry());

    // Serialize and write.
    let pretty = serde_json::to_string_pretty(&config)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

    std::fs::write(&config_path, pretty + "\n")
}

/// Remove Potato's MCP entry from `<project_dir>/.mcp.json`.
///
/// Removes both the shared `"potato"` entry and any legacy `potato-*` entries.
///
/// If the file becomes empty (no `mcpServers` entries remaining) or only
/// contains an empty `mcpServers` object, the file is deleted.
///
/// If the file does not exist, this is a no-op.
pub fn remove_mcp_config(project_dir: &Path) -> std::io::Result<()> {
    let config_path = mcp_config_path(project_dir);

    if !config_path.exists() {
        return Ok(());
    }

    let mut config = match load_config(&config_path) {
        Some(c) => c,
        None => return std::fs::remove_file(&config_path),
    };

    let servers = config
        .as_object_mut()
        .and_then(|o| o.get_mut("mcpServers"))
        .and_then(Value::as_object_mut);

    if let Some(servers) = servers {
        servers.remove("potato");
        servers.retain(|k, _| !k.starts_with("potato-"));
    }

    // If mcpServers is empty or missing, delete the file.
    let is_empty = config
        .as_object()
        .and_then(|o| o.get("mcpServers"))
        .and_then(Value::as_object)
        .map(|s| s.is_empty())
        .unwrap_or(true);

    if is_empty {
        // Also check if there are any other top-level keys besides mcpServers.
        let other_keys = config
            .as_object()
            .map(|o| o.keys().filter(|k| *k != "mcpServers").count())
            .unwrap_or(0);

        if other_keys == 0 {
            return std::fs::remove_file(&config_path);
        }
    }

    // Still has content — write back without potato-* entries.
    let pretty = serde_json::to_string_pretty(&config)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    std::fs::write(&config_path, pretty + "\n")
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn mcp_config_path(project_dir: &Path) -> PathBuf {
    project_dir.join(".mcp.json")
}

fn load_config(path: &Path) -> Option<Value> {
    let content = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&content).ok()
}

fn potato_server_entry() -> Value {
    // No env vars needed here — POTATO_PANE_ID and POTATO_SOCKET are
    // inherited from the parent Claude PTY process, which Potato sets
    // when spawning each pane. This means each Claude session's MCP
    // server process automatically knows which pane it belongs to.
    json!({
        "command": "potato",
        "args": ["mcp-server"]
    })
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// Create a unique temporary directory for a test. Returns the path.
    /// Caller is responsible for cleanup (or leaving it in /tmp).
    fn temp_test_dir(name: &str) -> PathBuf {
        let base = std::env::temp_dir().join(format!(
            "potato-mcp-test-{}-{}",
            name,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.subsec_nanos())
                .unwrap_or(0)
        ));
        fs::create_dir_all(&base).expect("create temp dir");
        base
    }

    fn read_config(dir: &Path) -> Value {
        let content = fs::read_to_string(dir.join(".mcp.json")).expect("config file");
        serde_json::from_str(&content).expect("valid JSON")
    }

    fn cleanup(dir: &Path) {
        let _ = fs::remove_dir_all(dir);
    }

    // ── write_mcp_config ──────────────────────────────────────────────────────

    #[test]
    fn creates_config_when_none_exists() {
        let dir = temp_test_dir("creates");
        write_mcp_config(&dir, &[], "").unwrap();
        let config = read_config(&dir);
        assert!(config["mcpServers"]["potato"].is_object());
        // No per-pane entries.
        let servers = config["mcpServers"].as_object().unwrap();
        assert!(!servers.keys().any(|k| k.starts_with("potato-")));
        cleanup(&dir);
    }

    #[test]
    fn written_config_has_correct_structure() {
        let dir = temp_test_dir("structure");
        write_mcp_config(&dir, &[], "").unwrap();
        let config = read_config(&dir);
        let entry = &config["mcpServers"]["potato"];
        assert_eq!(entry["command"], "potato");
        assert_eq!(entry["args"], json!(["mcp-server"]));
        // No env in config — inherited from parent process.
        assert!(entry.get("env").is_none());
        cleanup(&dir);
    }

    #[test]
    fn merges_with_existing_config() {
        let dir = temp_test_dir("merge");
        let existing = json!({
            "mcpServers": {
                "my-server": {
                    "command": "my-mcp",
                    "args": []
                }
            }
        });
        fs::write(
            dir.join(".mcp.json"),
            serde_json::to_string_pretty(&existing).unwrap(),
        ).unwrap();

        write_mcp_config(&dir, &[], "").unwrap();
        let config = read_config(&dir);

        // User's server preserved.
        assert!(config["mcpServers"]["my-server"].is_object());
        // Single potato entry added.
        assert!(config["mcpServers"]["potato"].is_object());
        cleanup(&dir);
    }

    #[test]
    fn cleans_legacy_per_pane_entries() {
        let dir = temp_test_dir("legacy");
        // Simulate old per-pane format.
        let legacy = json!({
            "mcpServers": {
                "potato-0": {"command": "potato", "args": ["mcp-server"], "env": {"POTATO_PANE_ID": "0"}},
                "potato-1": {"command": "potato", "args": ["mcp-server"], "env": {"POTATO_PANE_ID": "1"}}
            }
        });
        fs::write(
            dir.join(".mcp.json"),
            serde_json::to_string_pretty(&legacy).unwrap(),
        ).unwrap();

        write_mcp_config(&dir, &[], "").unwrap();
        let config = read_config(&dir);
        let servers = config["mcpServers"].as_object().unwrap();
        // Legacy entries gone.
        assert!(!servers.contains_key("potato-0"));
        assert!(!servers.contains_key("potato-1"));
        // Single shared entry present.
        assert!(servers.contains_key("potato"));
        cleanup(&dir);
    }

    #[test]
    fn idempotent_write() {
        let dir = temp_test_dir("idempotent");
        write_mcp_config(&dir, &[], "").unwrap();
        write_mcp_config(&dir, &[], "").unwrap();
        let config = read_config(&dir);
        let servers = config["mcpServers"].as_object().unwrap();
        // Exactly one potato entry.
        assert_eq!(servers.keys().filter(|k| k.starts_with("potato")).count(), 1);
        assert!(servers.contains_key("potato"));
        cleanup(&dir);
    }

    // ── remove_mcp_config ────────────────────────────────────────────────────

    #[test]
    fn remove_deletes_file_when_only_potato_entry() {
        let dir = temp_test_dir("remove_all");
        write_mcp_config(&dir, &[], "").unwrap();
        remove_mcp_config(&dir).unwrap();
        assert!(!dir.join(".mcp.json").exists());
        cleanup(&dir);
    }

    #[test]
    fn remove_cleans_legacy_and_shared_entries() {
        let dir = temp_test_dir("remove_legacy");
        let mixed = json!({
            "mcpServers": {
                "user-server": {"command": "my-cmd", "args": []},
                "potato": {"command": "potato", "args": ["mcp-server"]},
                "potato-0": {"command": "potato", "args": ["mcp-server"], "env": {}}
            }
        });
        fs::write(
            dir.join(".mcp.json"),
            serde_json::to_string_pretty(&mixed).unwrap(),
        ).unwrap();
        remove_mcp_config(&dir).unwrap();
        let config = read_config(&dir);
        assert!(config["mcpServers"]["user-server"].is_object());
        assert!(config["mcpServers"]["potato"].is_null());
        assert!(config["mcpServers"]["potato-0"].is_null());
        cleanup(&dir);
    }

    #[test]
    fn remove_is_noop_when_file_missing() {
        let dir = temp_test_dir("remove_noop");
        remove_mcp_config(&dir).unwrap();
        cleanup(&dir);
    }

    #[test]
    fn remove_then_write_is_clean() {
        let dir = temp_test_dir("remove_write");
        write_mcp_config(&dir, &[], "").unwrap();
        remove_mcp_config(&dir).unwrap();
        write_mcp_config(&dir, &[], "").unwrap();
        let config = read_config(&dir);
        assert!(config["mcpServers"]["potato"].is_object());
        cleanup(&dir);
    }

    #[test]
    fn write_returns_error_for_non_object_config() {
        let dir = temp_test_dir("non_object");
        // Write a JSON array instead of an object — should error, not panic.
        fs::write(dir.join(".mcp.json"), "[1, 2, 3]").unwrap();
        let result = write_mcp_config(&dir, &[], "");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("not a JSON object"),
            "unexpected error message: {err}"
        );
        cleanup(&dir);
    }

    #[test]
    fn write_returns_error_for_non_object_mcp_servers() {
        let dir = temp_test_dir("bad_servers");
        // mcpServers is a string instead of an object.
        let bad = json!({"mcpServers": "not-an-object"});
        fs::write(
            dir.join(".mcp.json"),
            serde_json::to_string_pretty(&bad).unwrap(),
        )
        .unwrap();
        let result = write_mcp_config(&dir, &[], "");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("not a JSON object"),
            "unexpected error message: {err}"
        );
        cleanup(&dir);
    }

    #[test]
    fn remove_leaves_file_with_other_top_level_keys() {
        let dir = temp_test_dir("remove_other_keys");
        let config = json!({
            "version": 1,
            "mcpServers": {
                "potato": {"command": "potato", "args": ["mcp-server"]}
            }
        });
        fs::write(
            dir.join(".mcp.json"),
            serde_json::to_string_pretty(&config).unwrap(),
        ).unwrap();
        remove_mcp_config(&dir).unwrap();
        let result = read_config(&dir);
        assert_eq!(result["version"], 1);
        assert!(result["mcpServers"]["potato"].is_null());
        cleanup(&dir);
    }
}
