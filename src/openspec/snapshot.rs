//! CLI-based OpenSpec snapshot — shells out to `openspec` to gather change data.
//!
//! Follows the same pattern as [`crate::git::GitSnapshot`]: synchronous `capture()`,
//! non-fatal on failure, periodic refresh from the main loop.

use std::process::Command;

/// Snapshot of OpenSpec project state, taken at startup and refreshed periodically.
#[derive(Debug, Clone, Default)]
pub struct OpenSpecSnapshot {
    /// Whether the `openspec` CLI binary was found on `$PATH`.
    pub cli_available: bool,
    /// All changes discovered via `openspec list --json`.
    pub changes: Vec<ChangeInfo>,
}

/// Summary of a single OpenSpec change (from `openspec list --json`).
#[derive(Debug, Clone, Default)]
pub struct ChangeInfo {
    /// Change directory name (e.g. `"bugfix-sweep"`).
    pub name: String,
    /// Number of completed tasks.
    pub completed_tasks: u32,
    /// Total number of tasks.
    pub total_tasks: u32,
    /// ISO-8601 last-modified timestamp.
    pub last_modified: String,
    /// Status string (e.g. `"in-progress"`, `"done"`).
    pub status: String,
    /// Artifact completion details (populated from `openspec status --change <name> --json`).
    pub artifacts: Vec<ArtifactInfo>,
}

/// An artifact within a change (from `openspec status --change <name> --json`).
#[derive(Debug, Clone, Default)]
pub struct ArtifactInfo {
    /// Artifact identifier (e.g. `"proposal"`, `"design"`, `"tasks"`).
    pub id: String,
    /// Status string (e.g. `"done"`, `"pending"`).
    pub status: String,
}

impl OpenSpecSnapshot {
    /// Capture a fresh snapshot by shelling out to the `openspec` CLI.
    ///
    /// Non-blocking but synchronous. Returns an empty snapshot if the CLI is
    /// missing or any command fails.
    pub fn capture() -> Self {
        let mut snap = Self::default();

        // Check if openspec is available.
        let Some(list_json) = openspec_output(&["list", "--json"]) else {
            return snap;
        };
        snap.cli_available = true;

        // Parse the list output.
        let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&list_json) else {
            return snap;
        };

        let Some(changes_arr) = parsed.get("changes").and_then(|v| v.as_array()) else {
            return snap;
        };

        snap.changes = changes_arr
            .iter()
            .filter_map(|v| {
                Some(ChangeInfo {
                    name: v.get("name")?.as_str()?.to_string(),
                    completed_tasks: v.get("completedTasks")?.as_u64()? as u32,
                    total_tasks: v.get("totalTasks")?.as_u64()? as u32,
                    last_modified: v
                        .get("lastModified")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    status: v
                        .get("status")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown")
                        .to_string(),
                    artifacts: Vec::new(),
                })
            })
            .collect();

        // Enrich the 5 most recent in-progress changes with artifact data.
        let enrichment_indices: Vec<usize> = snap
            .changes
            .iter()
            .enumerate()
            .filter(|(_, c)| c.status == "in-progress")
            .take(5)
            .map(|(i, _)| i)
            .collect();

        for idx in enrichment_indices {
            let name = snap.changes[idx].name.clone();
            if let Some(status_json) = openspec_output(&["status", "--change", &name, "--json"]) {
                if let Ok(status_val) = serde_json::from_str::<serde_json::Value>(&status_json) {
                    if let Some(artifacts) = status_val.get("artifacts").and_then(|v| v.as_array())
                    {
                        snap.changes[idx].artifacts = artifacts
                            .iter()
                            .filter_map(|a| {
                                Some(ArtifactInfo {
                                    id: a.get("id")?.as_str()?.to_string(),
                                    status: a
                                        .get("status")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("unknown")
                                        .to_string(),
                                })
                            })
                            .collect();
                    }
                }
            }
        }

        snap
    }
}

/// Run an `openspec` CLI command and return its trimmed stdout, or `None` on failure.
fn openspec_output(args: &[&str]) -> Option<String> {
    Command::new("openspec")
        .args(args)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_snapshot_is_safe() {
        let snap = OpenSpecSnapshot::default();
        assert!(!snap.cli_available);
        assert!(snap.changes.is_empty());
    }

    #[test]
    fn capture_does_not_panic() {
        // May or may not have openspec installed — should not panic either way.
        let _snap = OpenSpecSnapshot::capture();
    }

    #[test]
    fn parse_list_json() {
        let json = r#"{
            "changes": [
                {
                    "name": "bugfix-sweep",
                    "completedTasks": 1,
                    "totalTasks": 38,
                    "lastModified": "2026-03-30T01:52:16.803Z",
                    "status": "in-progress"
                },
                {
                    "name": "phase-6-commands",
                    "completedTasks": 2,
                    "totalTasks": 4,
                    "lastModified": "2026-03-30T01:51:21.425Z",
                    "status": "in-progress"
                }
            ]
        }"#;

        let parsed: serde_json::Value = serde_json::from_str(json).unwrap();
        let changes_arr = parsed.get("changes").unwrap().as_array().unwrap();
        let changes: Vec<ChangeInfo> = changes_arr
            .iter()
            .filter_map(|v| {
                Some(ChangeInfo {
                    name: v.get("name")?.as_str()?.to_string(),
                    completed_tasks: v.get("completedTasks")?.as_u64()? as u32,
                    total_tasks: v.get("totalTasks")?.as_u64()? as u32,
                    last_modified: v
                        .get("lastModified")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    status: v
                        .get("status")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown")
                        .to_string(),
                    artifacts: Vec::new(),
                })
            })
            .collect();

        assert_eq!(changes.len(), 2);
        assert_eq!(changes[0].name, "bugfix-sweep");
        assert_eq!(changes[0].completed_tasks, 1);
        assert_eq!(changes[0].total_tasks, 38);
        assert_eq!(changes[0].status, "in-progress");
        assert_eq!(changes[1].name, "phase-6-commands");
        assert_eq!(changes[1].completed_tasks, 2);
        assert_eq!(changes[1].total_tasks, 4);
    }

    #[test]
    fn parse_status_json_artifacts() {
        let json = r#"{
            "changeName": "openspec-cli-integration",
            "schemaName": "spec-driven",
            "isComplete": true,
            "artifacts": [
                {"id": "proposal", "outputPath": "proposal.md", "status": "done"},
                {"id": "design", "outputPath": "design.md", "status": "done"},
                {"id": "tasks", "outputPath": "tasks.md", "status": "pending"}
            ]
        }"#;

        let parsed: serde_json::Value = serde_json::from_str(json).unwrap();
        let artifacts: Vec<ArtifactInfo> = parsed
            .get("artifacts")
            .unwrap()
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|a| {
                Some(ArtifactInfo {
                    id: a.get("id")?.as_str()?.to_string(),
                    status: a
                        .get("status")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown")
                        .to_string(),
                })
            })
            .collect();

        assert_eq!(artifacts.len(), 3);
        assert_eq!(artifacts[0].id, "proposal");
        assert_eq!(artifacts[0].status, "done");
        assert_eq!(artifacts[2].id, "tasks");
        assert_eq!(artifacts[2].status, "pending");
    }

    #[test]
    fn partial_json_missing_fields_handled() {
        let json = r#"{"changes": [{"name": "test"}]}"#;
        let parsed: serde_json::Value = serde_json::from_str(json).unwrap();
        let changes_arr = parsed.get("changes").unwrap().as_array().unwrap();
        let changes: Vec<ChangeInfo> = changes_arr
            .iter()
            .filter_map(|v| {
                Some(ChangeInfo {
                    name: v.get("name")?.as_str()?.to_string(),
                    completed_tasks: v.get("completedTasks")?.as_u64()? as u32,
                    total_tasks: v.get("totalTasks")?.as_u64()? as u32,
                    ..Default::default()
                })
            })
            .collect();
        // Missing completedTasks/totalTasks → filter_map returns None → empty
        assert_eq!(changes.len(), 0);
    }

    #[test]
    fn empty_changes_array_handled() {
        let json = r#"{"changes": []}"#;
        let parsed: serde_json::Value = serde_json::from_str(json).unwrap();
        let changes_arr = parsed.get("changes").unwrap().as_array().unwrap();
        assert!(changes_arr.is_empty());
    }
}
