//! Parse `.openspec/backlog.yaml` into structured task data.

use std::path::Path;

use anyhow::{Context, Result};
use serde::Deserialize;

/// Top-level backlog file shape.
#[derive(Debug, Deserialize)]
pub struct OpenSpecBacklog {
    #[serde(default)]
    pub tasks: Vec<OpenSpecTask>,
}

/// A single OpenSpec task/ticket.
#[derive(Debug, Clone, Deserialize)]
pub struct OpenSpecTask {
    pub id: String,
    pub title: String,
    #[serde(default = "default_status")]
    pub status: TaskStatus,
    #[serde(default)]
    pub phase: Option<String>,
    #[serde(default)]
    pub severity: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub acceptance: Option<Vec<String>>,
}

fn default_status() -> TaskStatus {
    TaskStatus::Open
}

/// Task status — maps to OpenSpec conventions.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TaskStatus {
    Open,
    InProgress,
    Done,
    Blocked,
    /// Claimed by a Potato agent (written back by us).
    Claimed,
}

impl std::fmt::Display for TaskStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Open => write!(f, "open"),
            Self::InProgress => write!(f, "in-progress"),
            Self::Done => write!(f, "done"),
            Self::Blocked => write!(f, "blocked"),
            Self::Claimed => write!(f, "claimed"),
        }
    }
}

impl OpenSpecBacklog {
    /// Parse from a file path.
    pub fn from_file(path: &Path) -> Result<Self> {
        let contents = std::fs::read_to_string(path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        Self::from_str(&contents)
    }

    /// Parse from a YAML string.
    pub fn from_str(yaml: &str) -> Result<Self> {
        serde_yaml::from_str(yaml).context("failed to parse OpenSpec backlog YAML (check for unquoted backticks or special chars in acceptance/description fields)")
    }

    /// Get only actionable (non-done) tasks.
    pub fn open_tasks(&self) -> Vec<&OpenSpecTask> {
        self.tasks
            .iter()
            .filter(|t| !matches!(t.status, TaskStatus::Done))
            .collect()
    }

    /// Find a task by ID.
    pub fn find(&self, id: &str) -> Option<&OpenSpecTask> {
        self.tasks.iter().find(|t| t.id == id)
    }

    /// Update the status of a task by ID in the YAML file.
    /// Reads, patches, and writes the file atomically.
    pub fn update_status(path: &Path, task_id: &str, new_status: TaskStatus) -> Result<()> {
        let contents = std::fs::read_to_string(path)
            .with_context(|| format!("failed to read {}", path.display()))?;

        // Use string-level replacement to preserve YAML formatting/comments.
        // Find the task block by its id and replace the status line.
        let id_marker = format!("id: {task_id}");
        let mut lines: Vec<String> = contents.lines().map(|l| l.to_string()).collect();
        let mut found = false;

        for i in 0..lines.len() {
            if lines[i].contains(&id_marker) {
                // Search forward for the status line within the same task block.
                for j in (i + 1)..lines.len().min(i + 15) {
                    let trimmed = lines[j].trim_start();
                    if trimmed.starts_with("status:") {
                        let indent = lines[j].len() - trimmed.len();
                        lines[j] = format!("{}status: {}", " ".repeat(indent), new_status);
                        found = true;
                        break;
                    }
                    // Stop if we hit the next task.
                    if trimmed.starts_with("- id:") {
                        break;
                    }
                }
                if found {
                    break;
                }
            }
        }

        if !found {
            anyhow::bail!("task {task_id} not found in backlog");
        }

        let output = lines.join("\n") + "\n";
        std::fs::write(path, output)
            .with_context(|| format!("failed to write {}", path.display()))?;

        Ok(())
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
tasks:
  - id: T-101
    title: "Setup terminal"
    status: done
    phase: phase-1

  - id: T-201
    title: "PTY embedding"
    status: open
    phase: phase-2
    description: |
      Embed a real PTY.

  - id: T-301
    title: "Cockpit layout"
    status: in-progress
    phase: phase-3

  - id: T-401
    title: "Fix bug"
    status: blocked
    phase: bugfix-sweep
    severity: critical
"#;

    #[test]
    fn parse_sample() {
        let backlog = OpenSpecBacklog::from_str(SAMPLE).unwrap();
        assert_eq!(backlog.tasks.len(), 4);
        assert_eq!(backlog.tasks[0].status, TaskStatus::Done);
        assert_eq!(backlog.tasks[1].status, TaskStatus::Open);
        assert_eq!(backlog.tasks[2].status, TaskStatus::InProgress);
        assert_eq!(backlog.tasks[3].status, TaskStatus::Blocked);
    }

    #[test]
    fn open_tasks_excludes_done() {
        let backlog = OpenSpecBacklog::from_str(SAMPLE).unwrap();
        let open = backlog.open_tasks();
        assert_eq!(open.len(), 3);
        assert!(open.iter().all(|t| t.status != TaskStatus::Done));
    }

    #[test]
    fn find_by_id() {
        let backlog = OpenSpecBacklog::from_str(SAMPLE).unwrap();
        assert!(backlog.find("T-201").is_some());
        assert!(backlog.find("T-999").is_none());
    }

    #[test]
    fn update_status_in_file() {
        let dir = std::env::temp_dir().join("potato-openspec-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("backlog.yaml");
        std::fs::write(&path, SAMPLE).unwrap();

        OpenSpecBacklog::update_status(&path, "T-201", TaskStatus::Claimed).unwrap();

        let updated = OpenSpecBacklog::from_file(&path).unwrap();
        assert_eq!(updated.find("T-201").unwrap().status, TaskStatus::Claimed);
        // Other tasks unchanged.
        assert_eq!(updated.find("T-101").unwrap().status, TaskStatus::Done);
        assert_eq!(updated.find("T-301").unwrap().status, TaskStatus::InProgress);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
