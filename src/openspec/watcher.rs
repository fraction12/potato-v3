//! File watcher for `.openspec/backlog.yaml`.
//!
//! Emits reload events via a tokio channel when the backlog changes on disk.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use notify::{Event, EventKind, PollWatcher, RecursiveMode, Watcher};
use tokio::sync::mpsc;

use super::parser::OpenSpecBacklog;

/// Manages the backlog state and watches for file changes.
impl std::fmt::Debug for OpenSpecWatcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpenSpecWatcher")
            .field("path", &self.path)
            .finish_non_exhaustive()
    }
}

pub struct OpenSpecWatcher {
    /// Current parsed backlog (behind Arc+Mutex for cross-thread access).
    pub backlog: Arc<std::sync::Mutex<Option<OpenSpecBacklog>>>,
    /// Channel receiver — yields `()` on every file change.
    pub rx: mpsc::UnboundedReceiver<()>,
    /// Hold the watcher alive.
    _watcher: Option<PollWatcher>,
    /// Path to the backlog file.
    path: PathBuf,
}

impl OpenSpecWatcher {
    /// Create a watcher for the given project root.
    /// Looks for `<root>/.openspec/backlog.yaml`.
    /// Returns `None` if not found (project doesn't use OpenSpec).
    pub fn new(project_root: &Path) -> Option<Self> {
        let path = project_root.join(".openspec").join("backlog.yaml");
        if !path.exists() {
            tracing::info!("No .openspec/backlog.yaml in {} — project has no OpenSpec backlog", project_root.display());
            return None;
        }
        tracing::info!("Found OpenSpec backlog at {}", path.display());

        let (tx, rx) = mpsc::unbounded_channel();

        // Initial parse.
        let backlog = match OpenSpecBacklog::from_file(&path) {
            Ok(b) => {
                tracing::info!(
                    "Loaded OpenSpec backlog: {} tasks ({} open)",
                    b.tasks.len(),
                    b.open_tasks().len()
                );
                Some(b)
            }
            Err(e) => {
                tracing::warn!("Failed to parse OpenSpec backlog: {e}");
                None
            }
        };

        let backlog = Arc::new(std::sync::Mutex::new(backlog));

        // Set up file watcher.
        let watch_path = path.clone();
        let backlog_ref = Arc::clone(&backlog);
        let tx_clone = tx.clone();

        let watcher = Self::start_watcher(watch_path, backlog_ref, tx_clone);

        Some(Self {
            backlog,
            rx,
            _watcher: watcher,
            path,
        })
    }

    fn start_watcher(
        path: PathBuf,
        backlog: Arc<std::sync::Mutex<Option<OpenSpecBacklog>>>,
        tx: mpsc::UnboundedSender<()>,
    ) -> Option<PollWatcher> {
        let watch_dir = path.parent()?.to_path_buf();

        // T-905: Use PollWatcher instead of kqueue (RecommendedWatcher).
        // kqueue panics when watched paths are rapidly created/deleted
        // during heavy agent file ops. Polling every 2 seconds is fine
        // for a YAML that changes every few minutes.
        let config = notify::Config::default()
            .with_poll_interval(Duration::from_secs(2));

        let mut watcher = PollWatcher::new(
            move |res: Result<Event, notify::Error>| {
                let Ok(event) = res else { return };

                // Only react to modifications/creates of the backlog file.
                let dominated = matches!(
                    event.kind,
                    EventKind::Modify(_) | EventKind::Create(_)
                );
                if !dominated {
                    return;
                }

                let affects_backlog = event.paths.iter().any(|p| {
                    p.file_name()
                        .is_some_and(|n| n == "backlog.yaml")
                });
                if !affects_backlog {
                    return;
                }

                // Debounce: small sleep then reload.
                std::thread::sleep(Duration::from_millis(100));

                match OpenSpecBacklog::from_file(&path) {
                    Ok(b) => {
                        tracing::info!(
                            "OpenSpec backlog reloaded: {} tasks ({} open)",
                            b.tasks.len(),
                            b.open_tasks().len()
                        );
                        if let Ok(mut guard) = backlog.lock() {
                            *guard = Some(b);
                        }
                        let _ = tx.send(());
                    }
                    Err(e) => {
                        tracing::warn!("Failed to reload OpenSpec backlog: {e}");
                    }
                }
            },
            config,
        )
        .ok()?;

        if let Err(e) = watcher.watch(&watch_dir, RecursiveMode::NonRecursive) {
            tracing::warn!("Failed to watch .openspec/: {e}");
            return None;
        }

        tracing::info!("Watching .openspec/ for backlog changes (poll, 2s interval)");
        Some(watcher)
    }

    /// Get the backlog file path.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Update a task status on disk (triggers watcher reload).
    pub fn update_task_status(
        &self,
        task_id: &str,
        status: super::parser::TaskStatus,
    ) -> Result<()> {
        OpenSpecBacklog::update_status(&self.path, task_id, status)
            .with_context(|| format!("failed to update {task_id} in OpenSpec backlog"))
    }

    /// Snapshot the current open tasks (for UI rendering).
    pub fn open_tasks(&self) -> Vec<super::parser::OpenSpecTask> {
        self.backlog
            .lock()
            .ok()
            .and_then(|guard| {
                guard.as_ref().map(|b| {
                    b.open_tasks().into_iter().cloned().collect()
                })
            })
            .unwrap_or_default()
    }

    /// Find a task by ID in current backlog.
    pub fn find_task(&self, id: &str) -> Option<super::parser::OpenSpecTask> {
        self.backlog
            .lock()
            .ok()
            .and_then(|guard| {
                guard.as_ref().and_then(|b| b.find(id).cloned())
            })
    }

}
