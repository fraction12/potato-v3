//! Parse `openspec/changes/*/tasks.md` into structured task data.

use std::path::Path;

use anyhow::{Context, Result};

/// Top-level backlog, aggregated from all change directories.
#[derive(Debug)]
pub struct OpenSpecBacklog {
    pub tasks: Vec<OpenSpecTask>,
}

/// A single OpenSpec task/ticket.
#[derive(Debug, Clone)]
pub struct OpenSpecTask {
    pub id: String,
    pub title: String,
    pub status: TaskStatus,
    /// Derived from the change directory name (e.g. "bugfix-sweep").
    pub phase: Option<String>,
    /// From `[CRITICAL]`, `[HIGH]`, etc. — optional.
    pub severity: Option<String>,
    /// Text after ` — ` on the task line.
    pub description: Option<String>,
    /// Always `None` — tasks.md format has no acceptance criteria.
    pub acceptance: Option<Vec<String>>,
}

/// Task status — maps to OpenSpec conventions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskStatus {
    Open,
    InProgress,
    Done,
    Blocked,
    /// Claimed by a Potato agent.
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
    /// Scan all `<changes_dir>/*/tasks.md` files and parse them.
    /// Returns an empty backlog (not an error) if the directory is missing or empty.
    pub fn from_changes_dir(changes_dir: &Path) -> Result<Self> {
        if !changes_dir.exists() {
            return Ok(Self { tasks: Vec::new() });
        }

        let mut tasks = Vec::new();

        let entries = std::fs::read_dir(changes_dir)
            .with_context(|| format!("failed to read directory {}", changes_dir.display()))?;

        for entry in entries {
            let entry = entry.with_context(|| {
                format!("failed to read entry in {}", changes_dir.display())
            })?;

            let entry_path = entry.path();
            if !entry_path.is_dir() {
                continue;
            }

            // Phase name = directory name.
            let phase = entry_path
                .file_name()
                .and_then(|n| n.to_str())
                .map(|s| s.to_string());

            let tasks_md = entry_path.join("tasks.md");
            if !tasks_md.exists() {
                continue;
            }

            let content = std::fs::read_to_string(&tasks_md)
                .with_context(|| format!("failed to read {}", tasks_md.display()))?;

            let parsed = parse_tasks_md(&content, phase.as_deref());
            tasks.extend(parsed);
        }

        Ok(Self { tasks })
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
}

/// Parse a tasks.md string into a list of tasks.
/// `phase` is the change directory name (e.g. "bugfix-sweep").
fn parse_tasks_md(content: &str, phase: Option<&str>) -> Vec<OpenSpecTask> {
    let mut tasks = Vec::new();

    for line in content.lines() {
        let trimmed = line.trim();

        // Must start with `- [ ]` or `- [x]`
        let (status, rest) = if let Some(rest) = trimmed.strip_prefix("- [ ] ") {
            (TaskStatus::Open, rest)
        } else if let Some(rest) = trimmed.strip_prefix("- [x] ") {
            (TaskStatus::Done, rest)
        } else if let Some(rest) = trimmed.strip_prefix("- [X] ") {
            (TaskStatus::Done, rest)
        } else {
            continue;
        };

        // Task ID: first token before `:`
        let Some(colon_pos) = rest.find(':') else {
            continue;
        };
        let id = rest[..colon_pos].trim().to_string();
        if id.is_empty() {
            continue;
        }

        let after_colon = rest[colon_pos + 1..].trim();

        // Severity: optional bracketed word at start of after_colon
        let (severity, after_severity) = if after_colon.starts_with('[') {
            if let Some(close) = after_colon.find(']') {
                let sev = after_colon[1..close].to_string();
                let rest_after = after_colon[close + 1..].trim();
                (Some(sev), rest_after)
            } else {
                (None, after_colon)
            }
        } else {
            (None, after_colon)
        };

        // Split title and description on ` — ` (em-dash with spaces)
        // Also handle plain ` - ` as fallback? No — spec says ` — ` only.
        // Use the Unicode em dash U+2014.
        let em_dash = " \u{2014} ";
        let (title, description) = if let Some(pos) = after_severity.find(em_dash) {
            let t = after_severity[..pos].trim().to_string();
            let d = after_severity[pos + em_dash.len()..].trim().to_string();
            (t, Some(d).filter(|s| !s.is_empty()))
        } else {
            (after_severity.trim().to_string(), None)
        };

        if title.is_empty() {
            continue;
        }

        tasks.push(OpenSpecTask {
            id,
            title,
            status,
            phase: phase.map(|p| p.to_string()),
            severity,
            description,
            acceptance: None,
        });
    }

    tasks
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_TASKS_MD: &str = r#"# Tasks — Bugfix Sweep

- [ ] T-850: [CRITICAL] Fix carry bytes double-counted — carry bytes being added twice in accumulator
- [ ] T-1001: Remove bare-letter shortcuts — shortcut keys without modifier conflict with input
- [x] T-887: [LOW] Fix sparkline push — sparkline data not updating
- [ ] T-999: No severity no description
- [ ] T-777: [HIGH] Title only no em dash
"#;

    #[test]
    fn parse_well_formed_tasks_md() {
        let tasks = parse_tasks_md(SAMPLE_TASKS_MD, Some("bugfix-sweep"));
        assert_eq!(tasks.len(), 5);
    }

    #[test]
    fn open_and_done_status() {
        let tasks = parse_tasks_md(SAMPLE_TASKS_MD, Some("bugfix-sweep"));
        assert_eq!(tasks[0].status, TaskStatus::Open);   // T-850
        assert_eq!(tasks[1].status, TaskStatus::Open);   // T-1001
        assert_eq!(tasks[2].status, TaskStatus::Done);   // T-887
    }

    #[test]
    fn task_ids_parsed() {
        let tasks = parse_tasks_md(SAMPLE_TASKS_MD, Some("bugfix-sweep"));
        assert_eq!(tasks[0].id, "T-850");
        assert_eq!(tasks[1].id, "T-1001");
        assert_eq!(tasks[2].id, "T-887");
    }

    #[test]
    fn severity_extracted() {
        let tasks = parse_tasks_md(SAMPLE_TASKS_MD, Some("bugfix-sweep"));
        assert_eq!(tasks[0].severity.as_deref(), Some("CRITICAL"));
        assert_eq!(tasks[2].severity.as_deref(), Some("LOW"));
        assert_eq!(tasks[3].severity, None);  // T-999 has no severity
    }

    #[test]
    fn severity_high() {
        let tasks = parse_tasks_md(SAMPLE_TASKS_MD, Some("bugfix-sweep"));
        assert_eq!(tasks[4].severity.as_deref(), Some("HIGH"));  // T-777
    }

    #[test]
    fn description_after_em_dash() {
        let tasks = parse_tasks_md(SAMPLE_TASKS_MD, Some("bugfix-sweep"));
        assert_eq!(
            tasks[0].description.as_deref(),
            Some("carry bytes being added twice in accumulator")
        );
        assert_eq!(
            tasks[1].description.as_deref(),
            Some("shortcut keys without modifier conflict with input")
        );
    }

    #[test]
    fn missing_description_is_none() {
        let tasks = parse_tasks_md(SAMPLE_TASKS_MD, Some("bugfix-sweep"));
        // T-999 and T-777 have no em dash
        assert_eq!(tasks[3].description, None);
        assert_eq!(tasks[4].description, None);
    }

    #[test]
    fn title_without_severity() {
        let tasks = parse_tasks_md(SAMPLE_TASKS_MD, Some("bugfix-sweep"));
        // T-1001: no severity — title is everything up to em dash
        assert_eq!(tasks[1].title, "Remove bare-letter shortcuts");
    }

    #[test]
    fn title_with_severity() {
        let tasks = parse_tasks_md(SAMPLE_TASKS_MD, Some("bugfix-sweep"));
        assert_eq!(tasks[0].title, "Fix carry bytes double-counted");
    }

    #[test]
    fn phase_set_from_arg() {
        let tasks = parse_tasks_md(SAMPLE_TASKS_MD, Some("bugfix-sweep"));
        for t in &tasks {
            assert_eq!(t.phase.as_deref(), Some("bugfix-sweep"));
        }
    }

    #[test]
    fn non_checkbox_lines_ignored() {
        let content = r#"
# Header
Some text here
- Not a checkbox
  - nested non-checkbox
- [ ] T-1: Valid task — description
- [x] T-2: Done task
"#;
        let tasks = parse_tasks_md(content, None);
        assert_eq!(tasks.len(), 2);
        assert_eq!(tasks[0].id, "T-1");
        assert_eq!(tasks[1].id, "T-2");
    }

    #[test]
    fn acceptance_always_none() {
        let tasks = parse_tasks_md(SAMPLE_TASKS_MD, Some("bugfix-sweep"));
        for t in &tasks {
            assert!(t.acceptance.is_none());
        }
    }

    #[test]
    fn uppercase_x_done() {
        let content = "- [X] T-3: [MEDIUM] Upper-case X — should be done\n";
        let tasks = parse_tasks_md(content, None);
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].status, TaskStatus::Done);
    }

    #[test]
    fn open_tasks_excludes_done() {
        let backlog = OpenSpecBacklog {
            tasks: parse_tasks_md(SAMPLE_TASKS_MD, Some("bugfix-sweep")),
        };
        let open = backlog.open_tasks();
        // T-887 is done, so 4 tasks remain open
        assert_eq!(open.len(), 4);
        assert!(open.iter().all(|t| t.status != TaskStatus::Done));
    }

    #[test]
    fn find_by_id() {
        let backlog = OpenSpecBacklog {
            tasks: parse_tasks_md(SAMPLE_TASKS_MD, Some("bugfix-sweep")),
        };
        assert!(backlog.find("T-850").is_some());
        assert!(backlog.find("T-999").is_some());
        assert!(backlog.find("T-9999").is_none());
    }

    #[test]
    fn multiple_change_dirs_merged() {
        let tmp = tempfile::tempdir().unwrap();
        let changes = tmp.path().join("changes");

        // Create two change dirs with tasks.
        let sweep = changes.join("bugfix-sweep");
        std::fs::create_dir_all(&sweep).unwrap();
        std::fs::write(
            sweep.join("tasks.md"),
            "- [ ] T-1: Task one\n- [x] T-2: Done task\n",
        )
        .unwrap();

        let feat = changes.join("feature-x");
        std::fs::create_dir_all(&feat).unwrap();
        std::fs::write(
            feat.join("tasks.md"),
            "- [ ] T-3: [HIGH] Another task — desc\n",
        )
        .unwrap();

        let backlog = OpenSpecBacklog::from_changes_dir(&changes).unwrap();
        assert_eq!(backlog.tasks.len(), 3);

        // Check phases are set correctly
        let t1 = backlog.find("T-1").unwrap();
        assert_eq!(t1.phase.as_deref(), Some("bugfix-sweep"));
        let t3 = backlog.find("T-3").unwrap();
        assert_eq!(t3.phase.as_deref(), Some("feature-x"));
    }

    #[test]
    fn empty_changes_dir_returns_empty_backlog() {
        let tmp = tempfile::tempdir().unwrap();
        let changes = tmp.path().join("changes");
        std::fs::create_dir_all(&changes).unwrap();

        let backlog = OpenSpecBacklog::from_changes_dir(&changes).unwrap();
        assert_eq!(backlog.tasks.len(), 0);
    }

    #[test]
    fn missing_changes_dir_returns_empty_backlog() {
        let tmp = tempfile::tempdir().unwrap();
        let changes = tmp.path().join("does-not-exist");

        let backlog = OpenSpecBacklog::from_changes_dir(&changes).unwrap();
        assert_eq!(backlog.tasks.len(), 0);
    }

    #[test]
    fn task_status_display() {
        assert_eq!(TaskStatus::Open.to_string(), "open");
        assert_eq!(TaskStatus::InProgress.to_string(), "in-progress");
        assert_eq!(TaskStatus::Done.to_string(), "done");
        assert_eq!(TaskStatus::Blocked.to_string(), "blocked");
        assert_eq!(TaskStatus::Claimed.to_string(), "claimed");
    }

    #[test]
    fn from_changes_dir_real_if_present() {
        let path = std::path::Path::new("openspec/changes");
        if !path.exists() {
            eprintln!("Skipping: no real openspec/changes directory");
            return;
        }
        match OpenSpecBacklog::from_changes_dir(path) {
            Ok(b) => {
                let open = b.open_tasks().len();
                eprintln!("Real backlog: {} tasks, {} open", b.tasks.len(), open);
            }
            Err(e) => {
                panic!("Failed to parse real openspec/changes: {e:#}");
            }
        }
    }
}
