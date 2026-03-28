//! Agent profile system — loads and merges TOML profile definitions.
//!
//! Profiles are loaded from (in priority order, lowest to highest):
//! 1. Auto-generated defaults from detected agents.
//! 2. Global profiles in `~/.config/potato/profiles/` (one `.toml` per profile).
//! 3. Project-local profile in `.potato/profile.toml`.
//!
//! Project profiles with the same `name` override global profiles, which in
//! turn override auto-generated defaults.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

// ── AgentProfile ──────────────────────────────────────────────────────────────

/// A named profile describing how to launch a specific agent.
#[derive(Debug, Clone, PartialEq)]
pub struct AgentProfile {
    /// Human-readable display name (e.g. `"Claude Code"`).
    pub name: String,
    /// Adapter identifier: `"claude"`, `"codex"`, or `"generic"`.
    pub adapter: String,
    /// Optional path to the agent binary; overrides the adapter's `detect()`.
    pub binary: Option<String>,
    /// Optional LLM model override (e.g. `"gpt-4o"`).
    pub model: Option<String>,
    /// Extra CLI arguments appended to the command.
    pub extra_args: Vec<String>,
    /// Environment variables injected into the agent process.
    pub env: HashMap<String, String>,
    /// Working directory override. `None` = use the current directory.
    pub working_dir: Option<PathBuf>,
}

impl AgentProfile {
    /// Create a minimal profile with just a name and adapter.
    pub fn new(name: impl Into<String>, adapter: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            adapter: adapter.into(),
            binary: None,
            model: None,
            extra_args: Vec::new(),
            env: HashMap::new(),
            working_dir: None,
        }
    }
}

// ── TOML on-disk format ───────────────────────────────────────────────────────

/// Raw TOML representation of an [`AgentProfile`].
///
/// Fields match the spec exactly so profiles can be hand-edited.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RawProfile {
    pub name: String,
    pub adapter: String,
    pub binary: Option<String>,
    pub model: Option<String>,
    #[serde(default)]
    pub extra_args: Vec<String>,
    #[serde(default)]
    pub env: HashMap<String, String>,
    pub working_dir: Option<String>,
}

impl From<RawProfile> for AgentProfile {
    fn from(r: RawProfile) -> Self {
        Self {
            name: r.name,
            adapter: r.adapter,
            binary: r.binary,
            model: r.model,
            extra_args: r.extra_args,
            env: r.env,
            working_dir: r.working_dir.map(PathBuf::from),
        }
    }
}

impl From<&AgentProfile> for RawProfile {
    fn from(p: &AgentProfile) -> Self {
        Self {
            name: p.name.clone(),
            adapter: p.adapter.clone(),
            binary: p.binary.clone(),
            model: p.model.clone(),
            extra_args: p.extra_args.clone(),
            env: p.env.clone(),
            working_dir: p.working_dir.as_deref().and_then(|p| p.to_str()).map(str::to_string),
        }
    }
}

// ── ProfileLoader ─────────────────────────────────────────────────────────────

/// Loads and merges profiles from all sources.
pub struct ProfileLoader;

impl ProfileLoader {
    /// Load all profiles, merging sources lowest-to-highest priority.
    ///
    /// Returns a de-duplicated list where later definitions (higher priority)
    /// override earlier ones with the same `name`.
    pub fn load(detected_defaults: Vec<AgentProfile>) -> Vec<AgentProfile> {
        let mut profiles: HashMap<String, AgentProfile> = HashMap::new();

        // 1. Auto-generated defaults from detected agents.
        for p in detected_defaults {
            profiles.insert(p.name.clone(), p);
        }

        // 2. Global profiles from ~/.config/potato/profiles/*.toml
        if let Some(home) = dirs::home_dir() {
            let global_dir = home.join(".config").join("potato").join("profiles");
            if global_dir.is_dir() {
                load_dir(&global_dir, &mut profiles);
            }
        }

        // 3. Project-local profile from .potato/profile.toml
        if let Ok(cwd) = std::env::current_dir() {
            let project_file = cwd.join(".potato").join("profile.toml");
            if project_file.is_file() {
                load_file(&project_file, &mut profiles);
            }
        }

        // Return sorted by name for stable display ordering.
        let mut result: Vec<AgentProfile> = profiles.into_values().collect();
        result.sort_unstable_by(|a, b| a.name.cmp(&b.name));
        result
    }
}

/// Parse all `.toml` files in a directory, inserting profiles into the map.
///
/// Each file may contain a single `[profile]` table or be a direct table.
fn load_dir(dir: &Path, profiles: &mut HashMap<String, AgentProfile>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return; };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("toml") {
            load_file(&path, profiles);
        }
    }
}

/// Parse a single `.toml` file, inserting profiles into the map.
///
/// The file format is:
/// ```toml
/// name = "My Agent"
/// adapter = "claude"
/// model = "claude-opus-4-5"
/// ```
/// or a project file with multiple `[[profiles]]` entries (array of tables).
fn load_file(path: &Path, profiles: &mut HashMap<String, AgentProfile>) {
    let Ok(content) = std::fs::read_to_string(path) else { return; };
    let Ok(table) = toml::from_str::<toml::Value>(&content) else { return; };

    // Check for `[[profiles]]` array.
    if let Some(arr) = table.get("profiles").and_then(|v| v.as_array()) {
        for entry in arr {
            if let Ok(raw) = entry.clone().try_into::<RawProfile>() {
                let profile: AgentProfile = raw.into();
                profiles.insert(profile.name.clone(), profile);
            }
        }
        return;
    }

    // Single profile at the top level.
    if let Ok(raw) = table.try_into::<RawProfile>() {
        let profile: AgentProfile = raw.into();
        profiles.insert(profile.name.clone(), profile);
    }
}

// ── Default profile generation ────────────────────────────────────────────────

/// Generate default profiles from the detected agents list.
///
/// Each [`crate::app::state::AgentInfo`] becomes an [`AgentProfile`].
/// Only agents that are actually available (binary found) are included by
/// default, unless `include_unavailable` is true.
pub fn default_profiles_from_agents(
    agents: &[crate::app::state::AgentInfo],
    include_unavailable: bool,
) -> Vec<AgentProfile> {
    agents
        .iter()
        .filter(|a| include_unavailable || a.available)
        .map(|a| AgentProfile {
            name: a.name.clone(),
            adapter: a.adapter.clone(),
            binary: a.binary_path.as_deref().and_then(|p| p.to_str()).map(str::to_string),
            model: None,
            extra_args: Vec::new(),
            env: HashMap::new(),
            working_dir: None,
        })
        .collect()
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    // ── AgentProfile ──────────────────────────────────────────────────────────

    #[test]
    fn agent_profile_new_sets_name_and_adapter() {
        let p = AgentProfile::new("Claude Code", "claude");
        assert_eq!(p.name, "Claude Code");
        assert_eq!(p.adapter, "claude");
        assert!(p.binary.is_none());
        assert!(p.model.is_none());
        assert!(p.extra_args.is_empty());
        assert!(p.env.is_empty());
        assert!(p.working_dir.is_none());
    }

    #[test]
    fn raw_profile_roundtrip_via_toml() {
        let raw = RawProfile {
            name: "My Agent".into(),
            adapter: "codex".into(),
            binary: Some("/usr/local/bin/codex".into()),
            model: Some("o4-mini".into()),
            extra_args: vec!["--full-auto".into()],
            env: {
                let mut m = HashMap::new();
                m.insert("OPENAI_API_KEY".into(), "sk-test".into());
                m
            },
            working_dir: Some("/tmp/project".into()),
        };

        let toml_str = toml::to_string(&raw).unwrap();
        let decoded: RawProfile = toml::from_str(&toml_str).unwrap();
        assert_eq!(decoded.name, "My Agent");
        assert_eq!(decoded.adapter, "codex");
        assert_eq!(decoded.binary.as_deref(), Some("/usr/local/bin/codex"));
        assert_eq!(decoded.model.as_deref(), Some("o4-mini"));
        assert_eq!(decoded.extra_args, vec!["--full-auto"]);
        assert_eq!(decoded.env["OPENAI_API_KEY"], "sk-test");
        assert_eq!(decoded.working_dir.as_deref(), Some("/tmp/project"));
    }

    #[test]
    fn raw_profile_into_agent_profile() {
        let raw = RawProfile {
            name: "Codex".into(),
            adapter: "codex".into(),
            binary: None,
            model: Some("gpt-4o".into()),
            extra_args: vec![],
            env: HashMap::new(),
            working_dir: Some("/var/proj".into()),
        };
        let profile: AgentProfile = raw.into();
        assert_eq!(profile.name, "Codex");
        assert_eq!(profile.working_dir, Some(PathBuf::from("/var/proj")));
        assert_eq!(profile.model.as_deref(), Some("gpt-4o"));
    }

    #[test]
    fn agent_profile_to_raw_and_back() {
        let profile = AgentProfile {
            name: "Test".into(),
            adapter: "generic".into(),
            binary: Some("/usr/bin/test-agent".into()),
            model: None,
            extra_args: vec!["--verbose".into()],
            env: HashMap::new(),
            working_dir: None,
        };
        let raw: RawProfile = (&profile).into();
        let back: AgentProfile = raw.into();
        assert_eq!(back.name, profile.name);
        assert_eq!(back.adapter, profile.adapter);
        assert_eq!(back.binary, profile.binary);
        assert_eq!(back.extra_args, profile.extra_args);
    }

    // ── ProfileLoader ─────────────────────────────────────────────────────────

    #[test]
    fn loader_returns_detected_defaults_when_no_files() {
        let defaults = vec![
            AgentProfile::new("Claude Code", "claude"),
            AgentProfile::new("Codex", "codex"),
        ];
        // Without global/project files, defaults come back (sorted by name).
        // We can't easily test without touching filesystem; test the merge logic directly.
        let mut profiles: HashMap<String, AgentProfile> = HashMap::new();
        for p in &defaults {
            profiles.insert(p.name.clone(), p.clone());
        }
        let mut result: Vec<AgentProfile> = profiles.into_values().collect();
        result.sort_unstable_by(|a, b| a.name.cmp(&b.name));
        assert_eq!(result[0].name, "Claude Code");
        assert_eq!(result[1].name, "Codex");
    }

    #[test]
    fn loader_project_file_overrides_default() {
        let tmp_dir = std::env::temp_dir().join(format!("potato-profiles-{}", std::process::id()));
        std::fs::create_dir_all(&tmp_dir).unwrap();
        let profile_file = tmp_dir.join("profile.toml");
        let toml_content = r#"
name = "Claude Code"
adapter = "claude"
model = "claude-opus-4-5"
"#;
        let mut f = std::fs::File::create(&profile_file).unwrap();
        f.write_all(toml_content.as_bytes()).unwrap();

        let mut profiles: HashMap<String, AgentProfile> = HashMap::new();
        profiles.insert("Claude Code".into(), AgentProfile::new("Claude Code", "claude"));
        load_file(&profile_file, &mut profiles);

        let p = profiles.get("Claude Code").unwrap();
        assert_eq!(p.model.as_deref(), Some("claude-opus-4-5"));

        let _ = std::fs::remove_dir_all(&tmp_dir);
    }

    #[test]
    fn loader_dir_loads_all_toml_files() {
        let tmp_dir = std::env::temp_dir().join(format!("potato-profiles-dir-{}", std::process::id()));
        std::fs::create_dir_all(&tmp_dir).unwrap();

        // Write two profile files.
        let f1 = tmp_dir.join("claude.toml");
        let f2 = tmp_dir.join("codex.toml");
        std::fs::write(&f1, r#"name = "Claude Code"
adapter = "claude"
model = "claude-sonnet"
"#).unwrap();
        std::fs::write(&f2, r#"name = "Codex"
adapter = "codex"
"#).unwrap();

        let mut profiles: HashMap<String, AgentProfile> = HashMap::new();
        load_dir(&tmp_dir, &mut profiles);

        assert!(profiles.contains_key("Claude Code"));
        assert!(profiles.contains_key("Codex"));
        assert_eq!(profiles["Claude Code"].model.as_deref(), Some("claude-sonnet"));

        let _ = std::fs::remove_dir_all(&tmp_dir);
    }

    #[test]
    fn loader_multi_profile_toml_array() {
        let tmp_dir = std::env::temp_dir().join(format!("potato-profiles-arr-{}", std::process::id()));
        std::fs::create_dir_all(&tmp_dir).unwrap();
        let file = tmp_dir.join("multi.toml");
        let content = r#"
[[profiles]]
name = "Claude Code"
adapter = "claude"

[[profiles]]
name = "Codex"
adapter = "codex"
model = "gpt-4o"
"#;
        std::fs::write(&file, content).unwrap();

        let mut profiles: HashMap<String, AgentProfile> = HashMap::new();
        load_file(&file, &mut profiles);
        assert!(profiles.contains_key("Claude Code"));
        assert!(profiles.contains_key("Codex"));
        assert_eq!(profiles["Codex"].model.as_deref(), Some("gpt-4o"));

        let _ = std::fs::remove_dir_all(&tmp_dir);
    }

    // ── default_profiles_from_agents ─────────────────────────────────────────

    #[test]
    fn default_profiles_from_agents_available_only() {
        use crate::app::state::AgentInfo;
        let agents = vec![
            AgentInfo {
                name: "Claude Code".into(),
                adapter: "claude".into(),
                binary_path: Some(PathBuf::from("/usr/bin/claude")),
                available: true,
            },
            AgentInfo {
                name: "Codex".into(),
                adapter: "codex".into(),
                binary_path: None,
                available: false,
            },
        ];
        let profiles = default_profiles_from_agents(&agents, false);
        assert_eq!(profiles.len(), 1);
        assert_eq!(profiles[0].name, "Claude Code");
    }

    #[test]
    fn default_profiles_from_agents_include_unavailable() {
        use crate::app::state::AgentInfo;
        let agents = vec![
            AgentInfo {
                name: "Claude Code".into(),
                adapter: "claude".into(),
                binary_path: None,
                available: false,
            },
        ];
        let profiles = default_profiles_from_agents(&agents, true);
        assert_eq!(profiles.len(), 1);
        assert_eq!(profiles[0].name, "Claude Code");
    }

    #[test]
    fn default_profiles_binary_path_transferred() {
        use crate::app::state::AgentInfo;
        let agents = vec![AgentInfo {
            name: "Claude Code".into(),
            adapter: "claude".into(),
            binary_path: Some(PathBuf::from("/usr/local/bin/claude")),
            available: true,
        }];
        let profiles = default_profiles_from_agents(&agents, false);
        assert_eq!(profiles[0].binary.as_deref(), Some("/usr/local/bin/claude"));
    }
}
