//! Process management — spawns agent CLI processes and bridges their I/O
//! with Potato's canonical event system.
//!
//! Uses `tokio::process` with piped I/O (not portable-pty).

use std::sync::Arc;

use anyhow::{Context, Result};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::Child,
    sync::{broadcast, mpsc, watch},
};
use tracing::{debug, error, info, warn};

use crate::adapters::{AdapterConfig, AgentAdapter};
use crate::events::AgentEvent;

// ── PtyHandle ─────────────────────────────────────────────────────────────────

/// Public handle returned from [`PtyProcess::spawn`].
///
/// The handle owns sender/receiver ends of the I/O channels but not the
/// process itself (which runs as background tasks).
pub struct PtyHandle {
    /// Send text to the agent's stdin.
    pub stdin_tx: mpsc::Sender<String>,
    /// Receive parsed [`AgentEvent`]s broadcast by the reader task.
    pub event_rx: broadcast::Receiver<AgentEvent>,
    /// Watch the process exit code (`None` = still running).
    pub exit_rx: watch::Receiver<Option<i32>>,
}

// ── PtyProcess ────────────────────────────────────────────────────────────────

/// Spawns and manages an agent sub-process.
pub struct PtyProcess;

impl PtyProcess {
    /// Spawn an agent process and return a [`PtyHandle`] for communicating with it.
    ///
    /// Three background tasks are launched:
    /// 1. **Reader** — reads stdout lines, parses them via the adapter, broadcasts events.
    /// 2. **Writer** — receives strings from `stdin_tx`, writes them to the child stdin.
    /// 3. **Exit watcher** — waits for the child to exit, broadcasts `AgentExited`.
    pub async fn spawn(
        adapter: Arc<dyn AgentAdapter>,
        config: AdapterConfig,
    ) -> Result<PtyHandle> {
        let working_dir = config.working_dir.display().to_string();
        let adapter_name = adapter.name().to_string();
        let model = config.model.clone();

        // Build the command from the adapter.
        let mut cmd = adapter.build_command(&config);

        // We need piped stdin/stdout/stderr.
        cmd.stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());

        let mut child: Child = cmd.spawn().context("failed to spawn agent process")?;

        // Take ownership of the I/O handles.
        let stdout = child.stdout.take().context("no stdout handle")?;
        let stderr = child.stderr.take().context("no stderr handle")?;
        let mut stdin = child.stdin.take().context("no stdin handle")?;

        // Channels.
        let (event_tx, event_rx) = broadcast::channel::<AgentEvent>(1024);
        let (stdin_tx, mut stdin_rx) = mpsc::channel::<String>(256);
        let (exit_tx, exit_rx) = watch::channel::<Option<i32>>(None);

        // ── Emit AgentStarted ─────────────────────────────────────────────────
        let _ = event_tx.send(AgentEvent::AgentStarted {
            adapter: adapter_name.clone(),
            working_dir,
            model,
        });

        // ── Task 1: stdout reader ─────────────────────────────────────────────
        {
            let event_tx_r = event_tx.clone();
            let adapter_r = adapter.clone();
            let mut reader = BufReader::new(stdout).lines();

            tokio::spawn(async move {
                loop {
                    match reader.next_line().await {
                        Ok(Some(line)) => {
                            debug!(line = %line, "agent stdout");
                            let events = adapter_r.parse_line(&line);
                            for event in events {
                                let _ = event_tx_r.send(event);
                            }
                        }
                        Ok(None) => {
                            debug!("agent stdout EOF");
                            break;
                        }
                        Err(e) => {
                            error!(error = %e, "error reading agent stdout");
                            let _ = event_tx_r.send(AgentEvent::Error {
                                message: format!("stdout read error: {e}"),
                            });
                            break;
                        }
                    }
                }
            });
        }

        // ── Task 1b: stderr reader (emit as warnings) ─────────────────────────
        {
            let event_tx_e = event_tx.clone();
            let mut stderr_reader = BufReader::new(stderr).lines();

            tokio::spawn(async move {
                loop {
                    match stderr_reader.next_line().await {
                        Ok(Some(line)) => {
                            debug!(stderr = %line, "agent stderr");
                            if !line.trim().is_empty() {
                                let _ = event_tx_e.send(AgentEvent::Warning {
                                    message: line,
                                });
                            }
                        }
                        Ok(None) => break,
                        Err(e) => {
                            warn!(error = %e, "error reading agent stderr");
                            break;
                        }
                    }
                }
            });
        }

        // ── Task 2: stdin writer ──────────────────────────────────────────────
        tokio::spawn(async move {
            while let Some(text) = stdin_rx.recv().await {
                debug!(text = %text, "writing to agent stdin");
                if let Err(e) = stdin.write_all(text.as_bytes()).await {
                    error!(error = %e, "failed to write to agent stdin");
                    break;
                }
                if let Err(e) = stdin.flush().await {
                    error!(error = %e, "failed to flush agent stdin");
                    break;
                }
            }
            // Channel closed — drop stdin so the agent sees EOF.
        });

        // ── Task 3: exit watcher ──────────────────────────────────────────────
        {
            let event_tx_x = event_tx.clone();

            tokio::spawn(async move {
                let exit_status = child.wait().await;
                let exit_code = match exit_status {
                    Ok(status) => {
                        info!(code = ?status.code(), "agent process exited");
                        status.code()
                    }
                    Err(e) => {
                        error!(error = %e, "error waiting for agent process");
                        None
                    }
                };

                let _ = event_tx_x.send(AgentEvent::AgentExited { exit_code });
                let _ = exit_tx.send(Some(exit_code.unwrap_or(-1)));
            });
        }

        Ok(PtyHandle { stdin_tx, event_rx, exit_rx })
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::generic::GenericAdapter;
    use std::time::Duration;

    /// Verify that a simple process (echo) can be spawned and events received.
    #[tokio::test]
    async fn spawn_echo_process() {
        // Use a GenericAdapter backed by `echo`.
        let adapter = Arc::new(GenericAdapter::new("echo"));
        let config = AdapterConfig {
            working_dir: std::env::temp_dir(),
            extra_flags: vec!["hello from potato".to_string()],
            ..Default::default()
        };

        let handle = PtyProcess::spawn(adapter, config).await.unwrap();
        let mut rx = handle.event_rx;

        // Collect events for up to 2 seconds.
        let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
        let mut got_raw = false;

        loop {
            if tokio::time::Instant::now() >= deadline {
                break;
            }
            match tokio::time::timeout(Duration::from_millis(200), rx.recv()).await {
                Ok(Ok(AgentEvent::Raw { payload })) => {
                    if payload.contains("hello from potato") {
                        got_raw = true;
                    }
                }
                Ok(Ok(AgentEvent::AgentExited { .. })) => break,
                Ok(Ok(_)) => {}
                _ => break,
            }
        }

        assert!(got_raw, "expected Raw event with echo output");
    }

    #[tokio::test]
    async fn spawn_emits_agent_started() {
        let adapter = Arc::new(GenericAdapter::new("echo"));
        let config = AdapterConfig {
            working_dir: std::env::temp_dir(),
            extra_flags: vec!["test".to_string()],
            ..Default::default()
        };

        let handle = PtyProcess::spawn(adapter, config).await.unwrap();
        let mut rx = handle.event_rx;

        let first = tokio::time::timeout(Duration::from_secs(1), rx.recv())
            .await
            .expect("timeout")
            .expect("recv error");

        assert!(matches!(first, AgentEvent::AgentStarted { .. }));
    }

    #[test]
    fn pty_handle_has_expected_channels() {
        // Structural: just verify PtyHandle can be constructed (channels work).
        let (stdin_tx, _stdin_rx) = mpsc::channel::<String>(1);
        let (event_tx, event_rx) = broadcast::channel::<AgentEvent>(1);
        let (_exit_tx, exit_rx) = watch::channel::<Option<i32>>(None);
        drop(event_tx);
        let _handle = PtyHandle { stdin_tx, event_rx, exit_rx };
    }
}
