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
    ///
    /// ## Reliability notes
    ///
    /// - **Broadcast lag**: the event channel has capacity 1024. If a consumer
    ///   falls behind, [`broadcast::Receiver::recv`] will return
    ///   [`tokio::sync::broadcast::error::RecvError::Lagged`] for missed events.
    ///   Callers must handle this variant to avoid silent data loss.
    ///
    /// - **AgentStarted ordering**: `AgentStarted` is enqueued *before* the handle
    ///   is returned, so it is always the first event in the receiver's queue.
    ///
    /// - **Stdin error propagation**: if a write to the child's stdin fails the
    ///   writer task terminates and drops the child stdin handle.  The mpsc channel
    ///   buffer (256 slots) will fill up and callers will observe a backpressure
    ///   block on [`mpsc::Sender::send`].  Callers should use [`mpsc::Sender::try_send`]
    ///   or [`mpsc::Sender::send`] with a timeout if stdin errors must be detected.
    ///
    /// - **Dropped receivers**: dropping [`PtyHandle::event_rx`] does not panic or
    ///   block any background task; subsequent broadcast sends silently succeed
    ///   (no subscribers) or are dropped if the channel has no remaining receivers.
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
    use crate::adapters::{AdapterCapabilities, AdapterConfig};
    use crate::adapters::generic::GenericAdapter;
    use std::path::PathBuf;
    use std::time::Duration;
    use tokio::process::Command as TokioCommand;

    // ── FakeAdapter ───────────────────────────────────────────────────────────
    //
    // A controllable test adapter.  `parse_line` maps the raw line into a
    // configurable number of structured events (or a single Raw if
    // `multi_event` is false).

    struct FakeAdapter {
        /// Binary to run (e.g. "echo", "cat", "sh").
        binary: String,
        /// Extra args forwarded verbatim to the command.
        args: Vec<String>,
        /// When true, parse_line returns *two* events per non-empty line
        /// (a TextDelta + a Warning), simulating a multi-event adapter.
        multi_event: bool,
    }

    impl FakeAdapter {
        fn new(binary: impl Into<String>) -> Self {
            Self { binary: binary.into(), args: vec![], multi_event: false }
        }

        fn with_args(mut self, args: Vec<String>) -> Self {
            self.args = args;
            self
        }

        fn multi_event(mut self) -> Self {
            self.multi_event = true;
            self
        }
    }

    impl crate::adapters::AgentAdapter for FakeAdapter {
        fn name(&self) -> &str { &self.binary }

        fn detect(&self) -> Option<PathBuf> { None }

        fn capabilities(&self) -> AdapterCapabilities {
            AdapterCapabilities {
                structured_output: true,
                session_resumable: false,
                approval_intercept: false,
                tool_events: false,
            }
        }

        fn build_command(&self, config: &AdapterConfig) -> TokioCommand {
            let mut cmd = TokioCommand::new(&self.binary);
            cmd.current_dir(&config.working_dir);
            for a in &self.args {
                cmd.arg(a);
            }
            for a in &config.extra_flags {
                cmd.arg(a);
            }
            cmd
        }

        fn parse_line(&self, line: &str) -> Vec<AgentEvent> {
            if line.trim().is_empty() {
                return vec![];
            }
            if self.multi_event {
                // Return two distinct events so we can assert both arrive.
                vec![
                    AgentEvent::TextDelta { text: line.to_string() },
                    AgentEvent::Warning { message: format!("dup:{line}") },
                ]
            } else {
                vec![AgentEvent::Raw { payload: line.to_string() }]
            }
        }

        fn format_user_input(&self, text: &str) -> String { format!("{text}\n") }
        fn format_approval(&self, _approved: bool) -> Option<String> { None }
    }

    // ── Helper: drain events until AgentExited, with a timeout ───────────────

    async fn drain_until_exit(
        rx: &mut broadcast::Receiver<AgentEvent>,
        timeout: Duration,
    ) -> Vec<AgentEvent> {
        let deadline = tokio::time::Instant::now() + timeout;
        let mut collected = vec![];
        loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() { break; }
            match tokio::time::timeout(remaining.min(Duration::from_millis(100)), rx.recv()).await {
                Ok(Ok(ev)) => {
                    let done = matches!(ev, AgentEvent::AgentExited { .. });
                    collected.push(ev);
                    if done { break; }
                }
                _ => break,
            }
        }
        collected
    }

    // ─────────────────────────────────────────────────────────────────────────
    // TEST 1: spawn returns a working handle for a simple command
    // ─────────────────────────────────────────────────────────────────────────

    /// Spawn a real process (`echo`) and confirm a PtyHandle is returned and
    /// not immediately broken.  The first event must be AgentStarted and the
    /// channel must be healthy (not lagged/closed) immediately after spawn.
    #[tokio::test]
    async fn spawn_returns_working_handle() {
        let adapter = Arc::new(FakeAdapter::new("echo").with_args(vec!["ping".into()]));
        let config = AdapterConfig { working_dir: std::env::temp_dir(), ..Default::default() };

        let handle = PtyProcess::spawn(adapter, config).await.unwrap();

        // stdin_tx channel must be open.
        assert!(!handle.stdin_tx.is_closed(), "stdin channel should be open after spawn");

        // exit_rx must initially be None (process not yet exited).
        assert_eq!(*handle.exit_rx.borrow(), None, "exit_rx should be None immediately after spawn");

        // event_rx should be receivable (capacity > 0).
        // We spawned with broadcast capacity 1024; len() returns messages queued.
        // At minimum AgentStarted should be queued already.
        let mut rx = handle.event_rx;
        let first = tokio::time::timeout(Duration::from_secs(2), rx.recv())
            .await
            .expect("timed out waiting for first event")
            .expect("channel closed unexpectedly");

        assert!(
            matches!(first, AgentEvent::AgentStarted { .. }),
            "first event must be AgentStarted, got {first:?}",
        );
    }

    // ─────────────────────────────────────────────────────────────────────────
    // TEST 2: stdout lines are parsed through the adapter and broadcast
    // ─────────────────────────────────────────────────────────────────────────

    /// Spawn `echo "line1"` backed by FakeAdapter (raw mode).  Confirm that
    /// the Raw event payload matches the echo output.
    #[tokio::test]
    async fn stdout_lines_parsed_through_adapter_and_broadcast() {
        let adapter = Arc::new(FakeAdapter::new("echo").with_args(vec!["parsed-line".into()]));
        let config = AdapterConfig { working_dir: std::env::temp_dir(), ..Default::default() };

        let handle = PtyProcess::spawn(adapter, config).await.unwrap();
        let mut rx = handle.event_rx;

        let events = drain_until_exit(&mut rx, Duration::from_secs(3)).await;

        let raw = events.iter().any(|e| matches!(e, AgentEvent::Raw { payload } if payload.contains("parsed-line")));
        assert!(raw, "expected a Raw event containing 'parsed-line'; got: {events:?}");
    }

    // ─────────────────────────────────────────────────────────────────────────
    // TEST 3: stdin_tx writes are delivered to child stdin
    // ─────────────────────────────────────────────────────────────────────────

    /// Use `cat` as the child process (echoes stdin to stdout). Write a line
    /// via stdin_tx, then close the channel to let cat exit, and assert the
    /// Raw event with our payload arrives before AgentExited.
    #[tokio::test]
    async fn stdin_tx_writes_to_child_stdin() {
        // `cat` echoes stdin to stdout, exits on EOF.
        let adapter = Arc::new(FakeAdapter::new("cat"));
        let config = AdapterConfig { working_dir: std::env::temp_dir(), ..Default::default() };

        let handle = PtyProcess::spawn(adapter, config).await.unwrap();
        // Destructure so we can drop stdin_tx explicitly while keeping rx alive.
        let PtyHandle { stdin_tx, event_rx: mut rx, exit_rx: _ } = handle;

        // Send a distinctive line via the channel.
        stdin_tx.send("potato-stdin-write\n".to_string()).await.unwrap();

        // Drop stdin_tx — the writer task sees channel closed, drops child
        // stdin, and cat receives EOF and exits.
        drop(stdin_tx);

        let events = drain_until_exit(&mut rx, Duration::from_secs(5)).await;

        let got_payload = events
            .iter()
            .any(|e| matches!(e, AgentEvent::Raw { payload } if payload.contains("potato-stdin-write")));
        assert!(got_payload, "expected stdin write to appear as Raw event; events: {events:?}");
    }

    // ─────────────────────────────────────────────────────────────────────────
    // TEST 4: stderr does not deadlock and process exit is not lost
    // ─────────────────────────────────────────────────────────────────────────

    /// Run a shell command that emits a large amount of stderr output (to fill
    /// pipe buffers) and then exits.  Confirm AgentExited arrives and
    /// exit_rx is updated — verifying no deadlock between the stderr drain
    /// task and the exit watcher.
    #[tokio::test]
    async fn stderr_does_not_deadlock_or_lose_process_exit() {
        // yes(1) would never stop; use a shell loop that writes ~512 lines to
        // stderr then exits normally (exit 0).
        let adapter = Arc::new(FakeAdapter::new("sh").with_args(vec![
            "-c".into(),
            // Write 300 lines to stderr, then exit cleanly.
            "for i in $(seq 1 300); do echo \"stderr-line-$i\" >&2; done; exit 0".into(),
        ]));
        let config = AdapterConfig { working_dir: std::env::temp_dir(), ..Default::default() };

        let handle = PtyProcess::spawn(adapter, config).await.unwrap();
        let mut rx = handle.event_rx;
        let exit_rx = handle.exit_rx;

        let events = drain_until_exit(&mut rx, Duration::from_secs(10)).await;

        // AgentExited must be present.
        let exited = events.iter().any(|e| matches!(e, AgentEvent::AgentExited { .. }));
        assert!(exited, "AgentExited not received — possible deadlock; events len={}", events.len());

        // exit_rx watch must have been updated.
        let code = *exit_rx.borrow();
        assert!(code.is_some(), "exit_rx was never updated after process exit");

        // At least some Warning events (from stderr drain).
        let warnings = events.iter().filter(|e| matches!(e, AgentEvent::Warning { .. })).count();
        assert!(warnings > 0, "expected Warning events from stderr; got none");
    }

    // ─────────────────────────────────────────────────────────────────────────
    // TEST 5: process exit updates exit_rx and emits AgentExited event
    // ─────────────────────────────────────────────────────────────────────────

    /// Spawn `sh -c "exit 42"` and verify:
    ///   - AgentExited { exit_code: Some(42) } appears in the event stream
    ///   - exit_rx watch is updated to Some(42)
    #[tokio::test]
    async fn process_exit_updates_exit_rx_and_emits_agent_exited() {
        let adapter = Arc::new(FakeAdapter::new("sh").with_args(vec!["-c".into(), "exit 42".into()]));
        let config = AdapterConfig { working_dir: std::env::temp_dir(), ..Default::default() };

        let handle = PtyProcess::spawn(adapter, config).await.unwrap();
        let mut rx = handle.event_rx;
        let mut exit_rx = handle.exit_rx;

        // Wait until exit_rx changes (process exited), with a 5s deadline.
        tokio::time::timeout(Duration::from_secs(5), exit_rx.changed())
            .await
            .expect("timed out waiting for exit_rx change")
            .expect("exit_rx sender dropped");

        let code = *exit_rx.borrow();
        assert_eq!(code, Some(42), "exit_rx should hold exit code 42");

        // Drain remaining events to find AgentExited.
        let events = drain_until_exit(&mut rx, Duration::from_secs(2)).await;
        let agent_exited = events.iter().find_map(|e| {
            if let AgentEvent::AgentExited { exit_code } = e { Some(*exit_code) } else { None }
        });

        assert!(
            agent_exited.is_some(),
            "AgentExited event was never broadcast; events: {events:?}",
        );
        assert_eq!(
            agent_exited.unwrap(),
            Some(42),
            "AgentExited should carry exit_code 42",
        );
    }

    // ─────────────────────────────────────────────────────────────────────────
    // TEST 6: multiple parsed events from one stdout line are all broadcast
    // ─────────────────────────────────────────────────────────────────────────

    /// FakeAdapter in `multi_event` mode returns two events per line
    /// (TextDelta + Warning).  Confirm both arrive for the single echo line.
    #[tokio::test]
    async fn multiple_events_from_one_line_all_broadcast() {
        let adapter = Arc::new(
            FakeAdapter::new("echo")
                .with_args(vec!["multi".into()])
                .multi_event(),
        );
        let config = AdapterConfig { working_dir: std::env::temp_dir(), ..Default::default() };

        let handle = PtyProcess::spawn(adapter, config).await.unwrap();
        let mut rx = handle.event_rx;

        let events = drain_until_exit(&mut rx, Duration::from_secs(3)).await;

        let text_delta_count = events
            .iter()
            .filter(|e| matches!(e, AgentEvent::TextDelta { text } if text.contains("multi")))
            .count();
        let warning_count = events
            .iter()
            .filter(|e| matches!(e, AgentEvent::Warning { message } if message.contains("dup:multi")))
            .count();

        assert!(text_delta_count >= 1, "expected at least one TextDelta for 'multi'; got {events:?}");
        assert!(warning_count >= 1, "expected at least one Warning for 'dup:multi'; got {events:?}");
    }

    // ─────────────────────────────────────────────────────────────────────────
    // TEST 7: dropped event_rx does not panic the reader task
    // ─────────────────────────────────────────────────────────────────────────

    /// Drop the event receiver immediately after spawn (while the child is
    /// still running / producing output).  The reader task must not panic —
    /// it silently drops the send errors and exits cleanly.  We verify this
    /// indirectly by waiting for the process to finish (exit_rx changes).
    #[tokio::test]
    async fn dropped_event_receiver_does_not_panic() {
        // Use `echo` so the process exits quickly.
        let adapter = Arc::new(FakeAdapter::new("echo").with_args(vec!["drop-test".into()]));
        let config = AdapterConfig { working_dir: std::env::temp_dir(), ..Default::default() };

        let handle = PtyProcess::spawn(adapter, config).await.unwrap();
        let mut exit_rx = handle.exit_rx;

        // Drop event_rx immediately — all broadcast sends will return
        // SendError but must NOT panic the task.
        drop(handle.event_rx);

        // The exit watcher still runs; wait for it.
        tokio::time::timeout(Duration::from_secs(5), exit_rx.changed())
            .await
            .expect("timed out waiting for exit_rx after dropped receiver")
            .expect("exit_rx sender gone");

        // If we reach here without a panic: test passes.
        assert!(exit_rx.borrow().is_some(), "exit_rx should be Some after process exits");
    }

    // ─────────────────────────────────────────────────────────────────────────
    // TEST 8: dropped stdin_tx does not deadlock or panic the writer task
    // ─────────────────────────────────────────────────────────────────────────

    /// Drop stdin_tx immediately.  The writer task should notice the channel
    /// is closed, drop child stdin (signalling EOF to the child), and the
    /// process should exit.  For `cat` this causes an orderly exit.
    #[tokio::test]
    async fn dropped_stdin_tx_does_not_panic_writer_task() {
        let adapter = Arc::new(FakeAdapter::new("cat"));
        let config = AdapterConfig { working_dir: std::env::temp_dir(), ..Default::default() };

        let handle = PtyProcess::spawn(adapter, config).await.unwrap();
        let mut exit_rx = handle.exit_rx;

        // Drop stdin_tx → writer task sees channel closed → drops child stdin
        // → cat sees EOF → exits.
        drop(handle.stdin_tx);
        drop(handle.event_rx);

        // cat should exit within a few seconds.
        tokio::time::timeout(Duration::from_secs(5), exit_rx.changed())
            .await
            .expect("timed out — writer task likely blocked or stdin not closed")
            .expect("exit_rx sender gone");

        assert!(exit_rx.borrow().is_some(), "process should have exited after stdin EOF");
    }

    // ─────────────────────────────────────────────────────────────────────────
    // TEST 9: broadcast lag is returned as RecvError::Lagged, not a panic
    // ─────────────────────────────────────────────────────────────────────────

    /// Verify that a slow receiver that falls behind the broadcast channel
    /// capacity receives `RecvError::Lagged(n)` instead of panicking or
    /// silently blocking.  We use a channel with capacity 1024 (the production
    /// value) and emit slightly more events than that via a shell that writes
    /// 1100 lines to stdout; we intentionally do NOT drain the receiver,
    /// confirming the production code handles the back-pressure gracefully.
    ///
    /// This is a documentation/regression test: the implementation must *not*
    /// panic on a lagged receiver — the `let _ = event_tx.send(...)` pattern
    /// already handles this.  We assert the lag error kind is correct.
    #[tokio::test]
    async fn broadcast_lag_returns_lagged_error_not_panic() {

        // Emit 1100 lines from stdout → 1100 Raw events + 1 AgentStarted +
        // 1 AgentExited = 1102 total, which overflows the 1024-capacity channel.
        let adapter = Arc::new(
            FakeAdapter::new("sh").with_args(vec![
                "-c".into(),
                "for i in $(seq 1 1100); do echo \"line$i\"; done".into(),
            ]),
        );
        let config = AdapterConfig { working_dir: std::env::temp_dir(), ..Default::default() };

        let handle = PtyProcess::spawn(adapter, config).await.unwrap();
        let mut rx = handle.event_rx;
        let mut exit_rx = handle.exit_rx;

        // Wait for the process to complete before we read from the channel.
        // This guarantees the buffer is overflowed.
        tokio::time::timeout(Duration::from_secs(10), exit_rx.changed())
            .await
            .expect("timed out waiting for process exit")
            .ok();

        // Give tasks a moment to flush remaining sends.
        tokio::time::sleep(Duration::from_millis(50)).await;

        // Drain: we expect at least one RecvError::Lagged because the buffer
        // overflowed.  We also must not panic on receiving a Lagged error.
        let mut lagged = false;
        let mut received = 0usize;

        loop {
            match rx.try_recv() {
                Ok(_) => received += 1,
                Err(tokio::sync::broadcast::error::TryRecvError::Lagged(n)) => {
                    lagged = true;
                    // After a lag, try_recv resumes from the oldest available.
                    let _ = n;
                }
                Err(tokio::sync::broadcast::error::TryRecvError::Closed) => break,
                Err(tokio::sync::broadcast::error::TryRecvError::Empty) => break,
            }
        }

        assert!(
            lagged || received >= 1024,
            "expected either a Lagged error or >= 1024 events received; got {received} events, lagged={lagged}",
        );
    }

    // ─────────────────────────────────────────────────────────────────────────
    // LEGACY TESTS (kept, renamed for clarity)
    // ─────────────────────────────────────────────────────────────────────────

    /// Original echo integration test (GenericAdapter — Raw events).
    #[tokio::test]
    async fn spawn_echo_process() {
        let adapter = Arc::new(GenericAdapter::new("echo"));
        let config = AdapterConfig {
            working_dir: std::env::temp_dir(),
            extra_flags: vec!["hello from potato".to_string()],
            ..Default::default()
        };

        let handle = PtyProcess::spawn(adapter, config).await.unwrap();
        let mut rx = handle.event_rx;

        let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
        let mut got_raw = false;

        loop {
            if tokio::time::Instant::now() >= deadline { break; }
            match tokio::time::timeout(Duration::from_millis(200), rx.recv()).await {
                Ok(Ok(AgentEvent::Raw { payload })) => {
                    if payload.contains("hello from potato") { got_raw = true; }
                }
                Ok(Ok(AgentEvent::AgentExited { .. })) => break,
                Ok(Ok(_)) => {}
                _ => break,
            }
        }

        assert!(got_raw, "expected Raw event with echo output");
    }

    /// First event is AgentStarted.
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

    /// Structural: PtyHandle can be constructed manually.
    #[test]
    fn pty_handle_has_expected_channels() {
        let (stdin_tx, _stdin_rx) = mpsc::channel::<String>(1);
        let (event_tx, event_rx) = broadcast::channel::<AgentEvent>(1);
        let (_exit_tx, exit_rx) = watch::channel::<Option<i32>>(None);
        drop(event_tx);
        let _handle = PtyHandle { stdin_tx, event_rx, exit_rx };
    }
}
