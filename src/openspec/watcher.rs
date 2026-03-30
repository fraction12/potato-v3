//! File watcher for `openspec/changes/` directory.
//!
//! Emits reload events via a channel when any `tasks.md` changes on disk.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use notify::{Event, EventKind, PollWatcher, RecursiveMode, Watcher};
use tokio::sync::mpsc;

use super::parser::OpenSpecBacklog;

/// Manages the backlog state and watches for file changes.
pub struct OpenSpecWatcher {
    /// Current parsed backlog (behind Arc+Mutex for cross-thread access).
    pub backlog: Arc<std::sync::Mutex<Option<OpenSpecBacklog>>>,
    /// Channel receiver — yields `()` on every file change.
    pub rx: mpsc::UnboundedReceiver<()>,
    /// Hold the watcher alive.
    _watcher: Option<PollWatcher>,
    /// Path to the `openspec/changes/` directory.
    path: PathBuf,
}

impl std::fmt::Debug for OpenSpecWatcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpenSpecWatcher")
            .field("path", &self.path)
            .finish_non_exhaustive()
    }
}

impl OpenSpecWatcher {
    /// Create a watcher for the given project root.
    /// Looks for `<root>/openspec/changes/`.
    /// Returns `None` if not found (project doesn't use OpenSpec).
    pub fn new(project_root: &Path) -> Option<Self> {
        let changes_dir = project_root.join("openspec").join("changes");
        if !changes_dir.exists() {
            tracing::info!(
                "No openspec/changes/ in {} — project has no OpenSpec changes",
                project_root.display()
            );
            return None;
        }
        tracing::info!("Found OpenSpec changes at {}", changes_dir.display());

        let (tx, rx) = mpsc::unbounded_channel();

        // Initial parse.
        let backlog = match OpenSpecBacklog::from_changes_dir(&changes_dir) {
            Ok(b) => {
                tracing::info!(
                    "Loaded OpenSpec backlog: {} tasks ({} open)",
                    b.tasks.len(),
                    b.open_tasks().len()
                );
                Some(b)
            }
            Err(e) => {
                tracing::warn!("Failed to parse OpenSpec changes: {e}");
                None
            }
        };

        let backlog = Arc::new(std::sync::Mutex::new(backlog));

        let backlog_ref = Arc::clone(&backlog);
        let tx_clone = tx.clone();
        let changes_dir_clone = changes_dir.clone();

        let watcher = Self::start_watcher(changes_dir_clone, backlog_ref, tx_clone);

        Some(Self {
            backlog,
            rx,
            _watcher: watcher,
            path: changes_dir,
        })
    }

    fn start_watcher(
        changes_dir: PathBuf,
        backlog: Arc<std::sync::Mutex<Option<OpenSpecBacklog>>>,
        tx: mpsc::UnboundedSender<()>,
    ) -> Option<PollWatcher> {
        let config = notify::Config::default().with_poll_interval(Duration::from_secs(2));

        let watch_dir = changes_dir.clone();
        let mut watcher = PollWatcher::new(
            move |res: Result<Event, notify::Error>| {
                let Ok(event) = res else { return };

                // Only react to modifications/creates.
                let is_write = matches!(event.kind, EventKind::Modify(_) | EventKind::Create(_));
                if !is_write {
                    return;
                }

                // Only react when a tasks.md file is affected.
                let affects_tasks = event
                    .paths
                    .iter()
                    .any(|p| p.file_name().is_some_and(|n| n == "tasks.md"));
                if !affects_tasks {
                    return;
                }

                // Debounce: small sleep then reload.
                std::thread::sleep(Duration::from_millis(100));

                match OpenSpecBacklog::from_changes_dir(&watch_dir) {
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
                        tracing::warn!("Failed to reload OpenSpec changes: {e}");
                    }
                }
            },
            config,
        )
        .ok()?;

        if let Err(e) = watcher.watch(&changes_dir, RecursiveMode::Recursive) {
            tracing::warn!("Failed to watch openspec/changes/: {e}");
            return None;
        }

        tracing::info!("Watching openspec/changes/ for task changes (poll, 2s interval)");
        Some(watcher)
    }

    /// Get the `openspec/changes/` directory path.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Snapshot the current open tasks (for UI rendering).
    pub fn open_tasks(&self) -> Vec<super::parser::OpenSpecTask> {
        self.backlog
            .lock()
            .ok()
            .and_then(|guard| {
                guard
                    .as_ref()
                    .map(|b| b.open_tasks().into_iter().cloned().collect())
            })
            .unwrap_or_default()
    }
}
