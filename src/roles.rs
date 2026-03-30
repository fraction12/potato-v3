//! Role persistence — `.potato/roles.toml`.
//!
//! Roles define the identity and prompt each pane/agent receives at launch.
//! They persist per-project so users can configure teams once and reuse them.

use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::app::state::RoleDefinition;

/// On-disk representation of `.potato/roles.toml`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct RolesFile {
    #[serde(default)]
    roles: Vec<RoleDefinition>,
}

/// Load roles from `<project_root>/.potato/roles.toml`.
///
/// Returns an empty vec if the file is missing or malformed (logs a warning).
pub fn load_roles(project_root: &Path) -> Vec<RoleDefinition> {
    let path = project_root.join(".potato").join("roles.toml");
    if !path.exists() {
        return Vec::new();
    }

    match std::fs::read_to_string(&path) {
        Ok(content) => match toml::from_str::<RolesFile>(&content) {
            Ok(file) => {
                tracing::info!(
                    "Loaded {} role(s) from {}",
                    file.roles.len(),
                    path.display()
                );
                file.roles
            }
            Err(e) => {
                tracing::warn!("Failed to parse {}: {e}", path.display());
                Vec::new()
            }
        },
        Err(e) => {
            tracing::warn!("Failed to read {}: {e}", path.display());
            Vec::new()
        }
    }
}

/// Save roles to `<project_root>/.potato/roles.toml`.
pub fn save_roles(project_root: &Path, roles: &[RoleDefinition]) -> Result<()> {
    let dir = project_root.join(".potato");
    std::fs::create_dir_all(&dir).with_context(|| format!("failed to create {}", dir.display()))?;

    let file = RolesFile {
        roles: roles.to_vec(),
    };
    let content = toml::to_string_pretty(&file).context("failed to serialize roles")?;

    let path = dir.join("roles.toml");
    std::fs::write(&path, content)
        .with_context(|| format!("failed to write {}", path.display()))?;

    tracing::info!("Saved {} role(s) to {}", roles.len(), path.display());
    Ok(())
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn missing_file_returns_empty() {
        let tmp = TempDir::new().unwrap();
        let roles = load_roles(tmp.path());
        assert!(roles.is_empty());
    }

    #[test]
    fn round_trip() {
        let tmp = TempDir::new().unwrap();
        let roles = vec![
            RoleDefinition {
                name: "Architect".to_string(),
                prompt: "Design the system.".to_string(),
            },
            RoleDefinition {
                name: "Implementer".to_string(),
                prompt: "Write the code.".to_string(),
            },
        ];
        save_roles(tmp.path(), &roles).unwrap();
        let loaded = load_roles(tmp.path());
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].name, "Architect");
        assert_eq!(loaded[1].prompt, "Write the code.");
    }

    #[test]
    fn malformed_file_returns_empty() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join(".potato");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("roles.toml"), "this is not valid toml {{{").unwrap();
        let roles = load_roles(tmp.path());
        assert!(roles.is_empty());
    }

    #[test]
    fn empty_roles_array() {
        let tmp = TempDir::new().unwrap();
        save_roles(tmp.path(), &[]).unwrap();
        let loaded = load_roles(tmp.path());
        assert!(loaded.is_empty());
    }

    #[test]
    fn saved_file_is_valid_toml() {
        let tmp = TempDir::new().unwrap();
        let roles = vec![RoleDefinition {
            name: "Reviewer".to_string(),
            prompt: "Review PRs carefully.".to_string(),
        }];
        save_roles(tmp.path(), &roles).unwrap();

        let content =
            std::fs::read_to_string(tmp.path().join(".potato").join("roles.toml")).unwrap();
        assert!(content.contains("[[roles]]"));
        assert!(content.contains("Reviewer"));
    }
}
