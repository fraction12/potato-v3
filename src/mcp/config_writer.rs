//! Dynamic `.mcp.json` lifecycle management for Potato.
//!
//! Writes per-pane MCP server entries into `.mcp.json` in the project directory,
//! merging with any existing user-defined MCP servers. Only touches `potato-*` keys.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde_json::{json, Map, Value};

// ── Public API ────────────────────────────────────────────────────────────────

/// Write (or merge) Potato's MCP server entries into `<project_dir>/.mcp.json`.
///
/// Each pane gets its own entry: `potato-<pane_id>`. The `socket_path` is
/// passed to each server process via `POTATO_SOCKET` env var.
///
/// Existing entries that are NOT `potato-*` are preserved unchanged.
pub fn write_mcp_config(
    project_dir: &Path,
    pane_ids: &[u64],
    socket_path: &str,
) -> std::io::Result<()> {
    let config_path = mcp_config_path(project_dir);

    // Load existing config (or start with empty object).
    let mut config = load_config(&config_path).unwrap_or_else(|| json!({}));

    // Ensure mcpServers key exists.
    let servers = config
        .as_object_mut()
        .expect("config must be a JSON object")
        .entry("mcpServers")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .expect("mcpServers must be a JSON object");

    // Remove stale potato-* entries.
    servers.retain(|k, _| !k.starts_with("potato-"));

    // Add one entry per pane.
    for &pane_id in pane_ids {
        let key = format!("potato-{pane_id}");
        servers.insert(key, potato_server_entry(pane_id, socket_path));
    }

    // Serialize and write.
    let pretty = serde_json::to_string_pretty(&config)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

    std::fs::write(&config_path, pretty + "\n")
}

/// Remove all `potato-*` entries from `<project_dir>/.mcp.json`.
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

fn potato_server_entry(pane_id: u64, socket_path: &str) -> Value {
    json!({
        "command": "potato",
        "args": ["mcp-server"],
        "env": {
            "POTATO_PANE_ID": pane_id.to_string(),
            "POTATO_SOCKET": socket_path
        }
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
        write_mcp_config(&dir, &[0, 1], "/tmp/potato-1234.sock").unwrap();
        let config = read_config(&dir);
        assert!(config["mcpServers"]["potato-0"].is_object());
        assert!(config["mcpServers"]["potato-1"].is_object());
        cleanup(&dir);
    }

    #[test]
    fn written_config_has_correct_structure() {
        let dir = temp_test_dir("structure");
        write_mcp_config(&dir, &[0], "/tmp/potato-999.sock").unwrap();
        let config = read_config(&dir);
        let entry = &config["mcpServers"]["potato-0"];
        assert_eq!(entry["command"], "potato");
        assert_eq!(entry["args"], json!(["mcp-server"]));
        assert_eq!(entry["env"]["POTATO_PANE_ID"], "0");
        assert_eq!(entry["env"]["POTATO_SOCKET"], "/tmp/potato-999.sock");
        cleanup(&dir);
    }

    #[test]
    fn merges_with_existing_config() {
        let dir = temp_test_dir("merge");
        // Pre-populate with a user's MCP server.
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

        write_mcp_config(&dir, &[0], "/tmp/potato.sock").unwrap();
        let config = read_config(&dir);

        // User's server preserved.
        assert!(config["mcpServers"]["my-server"].is_object());
        // Potato entry added.
        assert!(config["mcpServers"]["potato-0"].is_object());
        cleanup(&dir);
    }

    #[test]
    fn overwrites_stale_potato_entries() {
        let dir = temp_test_dir("stale");
        // Write old potato-0 and potato-1.
        write_mcp_config(&dir, &[0, 1], "/tmp/old.sock").unwrap();
        // Now only pane 0 exists (pane 1 closed).
        write_mcp_config(&dir, &[0], "/tmp/new.sock").unwrap();
        let config = read_config(&dir);
        assert!(config["mcpServers"]["potato-0"].is_object());
        assert!(config["mcpServers"]["potato-1"].is_null());
        // Socket updated.
        assert_eq!(config["mcpServers"]["potato-0"]["env"]["POTATO_SOCKET"], "/tmp/new.sock");
        cleanup(&dir);
    }

    #[test]
    fn idempotent_write() {
        let dir = temp_test_dir("idempotent");
        write_mcp_config(&dir, &[0, 1], "/tmp/potato.sock").unwrap();
        write_mcp_config(&dir, &[0, 1], "/tmp/potato.sock").unwrap();
        let config = read_config(&dir);
        // Still exactly two potato entries.
        let servers = config["mcpServers"].as_object().unwrap();
        let potato_count = servers.keys().filter(|k| k.starts_with("potato-")).count();
        assert_eq!(potato_count, 2);
        cleanup(&dir);
    }

    #[test]
    fn write_single_pane() {
        let dir = temp_test_dir("single");
        write_mcp_config(&dir, &[5], "/tmp/s.sock").unwrap();
        let config = read_config(&dir);
        assert!(config["mcpServers"]["potato-5"].is_object());
        assert_eq!(config["mcpServers"]["potato-5"]["env"]["POTATO_PANE_ID"], "5");
        cleanup(&dir);
    }

    #[test]
    fn write_zero_panes_removes_potato_entries() {
        let dir = temp_test_dir("zero_panes");
        write_mcp_config(&dir, &[0, 1], "/tmp/s.sock").unwrap();
        write_mcp_config(&dir, &[], "/tmp/s.sock").unwrap();
        let config = read_config(&dir);
        let servers = config["mcpServers"].as_object().unwrap();
        assert!(!servers.keys().any(|k| k.starts_with("potato-")));
        cleanup(&dir);
    }

    // ── remove_mcp_config ────────────────────────────────────────────────────

    #[test]
    fn remove_deletes_file_when_only_potato_entries() {
        let dir = temp_test_dir("remove_all");
        write_mcp_config(&dir, &[0, 1], "/tmp/s.sock").unwrap();
        remove_mcp_config(&dir).unwrap();
        assert!(!dir.join(".mcp.json").exists());
        cleanup(&dir);
    }

    #[test]
    fn remove_preserves_user_servers() {
        let dir = temp_test_dir("remove_preserve");
        let mixed = json!({
            "mcpServers": {
                "user-server": {"command": "my-cmd", "args": []},
                "potato-0": {"command": "potato", "args": ["mcp-server"], "env": {}}
            }
        });
        fs::write(
            dir.join(".mcp.json"),
            serde_json::to_string_pretty(&mixed).unwrap(),
        ).unwrap();
        remove_mcp_config(&dir).unwrap();
        // File should still exist.
        let config = read_config(&dir);
        assert!(config["mcpServers"]["user-server"].is_object());
        assert!(config["mcpServers"]["potato-0"].is_null());
        cleanup(&dir);
    }

    #[test]
    fn remove_is_noop_when_file_missing() {
        let dir = temp_test_dir("remove_noop");
        // No error when file doesn't exist.
        remove_mcp_config(&dir).unwrap();
        cleanup(&dir);
    }

    #[test]
    fn remove_then_write_is_clean() {
        let dir = temp_test_dir("remove_write");
        write_mcp_config(&dir, &[0], "/tmp/s.sock").unwrap();
        remove_mcp_config(&dir).unwrap();
        write_mcp_config(&dir, &[0, 1], "/tmp/s2.sock").unwrap();
        let config = read_config(&dir);
        assert!(config["mcpServers"]["potato-0"].is_object());
        assert!(config["mcpServers"]["potato-1"].is_object());
        cleanup(&dir);
    }

    #[test]
    fn remove_leaves_file_with_other_top_level_keys() {
        let dir = temp_test_dir("remove_other_keys");
        let config = json!({
            "version": 1,
            "mcpServers": {
                "potato-0": {"command": "potato", "args": ["mcp-server"], "env": {}}
            }
        });
        fs::write(
            dir.join(".mcp.json"),
            serde_json::to_string_pretty(&config).unwrap(),
        ).unwrap();
        remove_mcp_config(&dir).unwrap();
        // File should still exist because of the "version" key.
        let result = read_config(&dir);
        assert_eq!(result["version"], 1);
        assert!(result["mcpServers"]["potato-0"].is_null());
        cleanup(&dir);
    }
}
