//! Git repository information for the left-rail panel.
//!
//! Shells out to `git` (and optionally `gh`) to gather branch, commit, and PR data.
//! All operations are non-fatal — if git isn't available or CWD isn't a repo,
//! the snapshot is simply empty.

use std::process::Command;

/// Snapshot of git repository state, taken at startup and refreshed periodically.
#[derive(Debug, Clone, Default)]
pub struct GitSnapshot {
    /// Whether CWD is inside a git repo.
    pub is_repo: bool,
    /// Current branch name (or detached HEAD short sha).
    pub current_branch: String,
    /// Local branches (name, is_current).
    pub branches: Vec<BranchInfo>,
    /// Recent commits on current branch (newest first).
    pub recent_commits: Vec<CommitInfo>,
    /// Open pull requests (via `gh`, empty if `gh` unavailable).
    pub open_prs: Vec<PrInfo>,
    /// Uncommitted file count (staged + unstaged).
    pub dirty_count: usize,
    /// Short status lines (e.g. "M src/main.rs").
    pub status_lines: Vec<String>,
}

/// A local git branch.
#[derive(Debug, Clone)]
pub struct BranchInfo {
    /// Branch name (e.g. `main`, `feature/foo`).
    pub name: String,
    /// `true` when this is the currently checked-out branch.
    pub is_current: bool,
}

/// A single commit from `git log`.
#[derive(Debug, Clone)]
pub struct CommitInfo {
    /// Abbreviated commit hash (7 chars).
    pub short_sha: String,
    /// First line of the commit message.
    pub message: String,
    /// Author name.
    pub author: String,
    /// Human-readable relative date (e.g. "3 hours ago").
    pub relative_date: String,
}

/// An open pull request retrieved via `gh pr list`.
#[derive(Debug, Clone)]
pub struct PrInfo {
    /// PR number on GitHub.
    pub number: u32,
    /// PR title.
    pub title: String,
    /// GitHub login of the PR author.
    pub author: String,
    /// Head branch name.
    pub branch: String,
}

impl GitSnapshot {
    /// Capture a fresh snapshot. Non-blocking but synchronous (suited for startup
    /// or a background refresh task).
    pub fn capture() -> Self {
        let mut snap = Self::default();

        // Check if we're in a repo.
        let Ok(output) = Command::new("git")
            .args(["rev-parse", "--is-inside-work-tree"])
            .output()
        else {
            return snap;
        };
        if !output.status.success() {
            return snap;
        }
        snap.is_repo = true;

        // Current branch.
        snap.current_branch = git_output(&["rev-parse", "--abbrev-ref", "HEAD"])
            .unwrap_or_default();

        // Branches.
        if let Some(raw) = git_output(&["branch", "--format=%(refname:short) %(HEAD)"]) {
            snap.branches = raw
                .lines()
                .filter(|l| !l.is_empty())
                .map(|line| {
                    let is_current = line.ends_with('*');
                    let name = line.trim_end_matches(" *").trim_end().to_string();
                    BranchInfo { name, is_current }
                })
                .collect();
        }

        // Recent commits (last 8).
        if let Some(raw) = git_output(&[
            "log",
            "--oneline",
            "--format=%h\x1f%s\x1f%an\x1f%ar",
            "-8",
        ]) {
            snap.recent_commits = raw
                .lines()
                .filter(|l| !l.is_empty())
                .filter_map(|line| {
                    let parts: Vec<&str> = line.splitn(4, '\x1f').collect();
                    if parts.len() == 4 {
                        Some(CommitInfo {
                            short_sha: parts[0].to_string(),
                            message: parts[1].to_string(),
                            author: parts[2].to_string(),
                            relative_date: parts[3].to_string(),
                        })
                    } else {
                        None
                    }
                })
                .collect();
        }

        // Dirty status.
        if let Some(raw) = git_output(&["status", "--porcelain", "--short"]) {
            snap.status_lines = raw
                .lines()
                .filter(|l| !l.is_empty())
                .map(|l| l.to_string())
                .collect();
            snap.dirty_count = snap.status_lines.len();
        }

        // Open PRs via gh (best-effort).
        if let Some(raw) = gh_output(&[
            "pr", "list", "--state", "open", "--limit", "5",
            "--json", "number,title,author,headRefName",
        ]) {
            if let Ok(prs) = serde_json::from_str::<Vec<serde_json::Value>>(&raw) {
                snap.open_prs = prs
                    .iter()
                    .filter_map(|v| {
                        Some(PrInfo {
                            number: v.get("number")?.as_u64()? as u32,
                            title: v.get("title")?.as_str()?.to_string(),
                            author: v
                                .get("author")
                                .and_then(|a| a.get("login"))
                                .and_then(|l| l.as_str())
                                .unwrap_or("unknown")
                                .to_string(),
                            branch: v.get("headRefName")?.as_str()?.to_string(),
                        })
                    })
                    .collect();
            }
        }

        snap
    }
}

/// Run a `git` command and return its trimmed stdout, or `None` on failure.
fn git_output(args: &[&str]) -> Option<String> {
    Command::new("git")
        .args(args)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
}

/// Run a `gh` CLI command and return its trimmed stdout, or `None` on failure.
fn gh_output(args: &[&str]) -> Option<String> {
    Command::new("gh")
        .args(args)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_snapshot_is_safe() {
        let snap = GitSnapshot::default();
        assert!(!snap.is_repo);
        assert!(snap.branches.is_empty());
        assert!(snap.recent_commits.is_empty());
        assert!(snap.open_prs.is_empty());
        assert_eq!(snap.dirty_count, 0);
    }

    #[test]
    fn capture_does_not_panic() {
        // May or may not be in a repo — should not panic either way.
        let _snap = GitSnapshot::capture();
    }
}
