//! Potato — terminal cockpit for external coding agents.
//!
//! Boots to a dashboard where you pick an agent, then suspends its TUI,
//! hands the full terminal to the agent, and reclaims it when the agent exits.

// Scaffold: suppress warnings for types/items not yet fully wired.
#![allow(dead_code, unused_imports, unused_variables)]

mod adapters;
mod app;
mod claude_log;
mod codex_log;
mod config;
mod events;
mod git;
mod input;
mod log;
mod mcp;
mod metrics;
mod openspec;
mod roles;
mod pty;
mod session;
mod terminal;
mod ui;

use std::io::{self, Write as _};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use clap::{Parser, Subcommand};
use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture, KeyCode, KeyModifiers, MouseEventKind},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::DefaultTerminal;
use uuid::Uuid;

use app::message::Message;
use app::state::{AppScreen, AppState, CockpitFocus, DashboardFocus};
use app::update::update;
use config::load_config;
use session::{SessionStore, discover_historical_sessions, unix_now};
use terminal::events::event_stream;
use terminal::panic_hook::install_panic_hook;
use adapters::{AgentAdapter, claude::ClaudeAdapter, codex::CodexAdapter, generic::GenericAdapter};
use app::state::{AgentInfo, DashboardState};
use ui::screens::{dashboard::render_dashboard, session::render_session};
use crate::pty::{TurnHandle, key_event_to_bytes};

// ── CLI arguments ─────────────────────────────────────────────────────────────

/// Potato — terminal cockpit for external coding agents.
#[derive(Parser, Debug)]
#[command(name = "potato", version, about)]
struct Cli {
    /// Agent adapter to launch (claude|generic). Overrides dashboard selection.
    #[arg(short, long)]
    agent: Option<String>,

    /// Working directory for the agent session.
    #[arg(short, long)]
    workdir: Option<String>,

    /// LLM model to use (passed to the agent adapter).
    #[arg(short, long)]
    model: Option<String>,

    /// Path to a custom config file.
    #[arg(short, long)]
    config: Option<String>,

    /// Subcommand (if absent, runs the TUI as normal).
    #[command(subcommand)]
    command: Option<PotatoCommand>,
}

/// Optional subcommands for Potato.
#[derive(Subcommand, Debug)]
enum PotatoCommand {
    /// Run as a per-pane MCP stdio server (launched by Claude via .mcp.json).
    ///
    /// Reads JSON-RPC lines from stdin, forwards them to the main Potato
    /// process over a Unix domain socket, and writes responses to stdout.
    ///
    /// Environment variables (required):
    ///   POTATO_PANE_ID  — which pane this instance represents (u64)
    ///   POTATO_SOCKET   — path to the main process UDS socket
    #[command(name = "mcp-server")]
    McpServer,
}

// ── MCP server subprocess entry point ─────────────────────────────────────────

/// Run as a per-pane MCP stdio server.
///
/// Reads JSON-RPC lines from stdin, wraps them in a `BridgeRequest` with the
/// pane id from `POTATO_PANE_ID`, sends them over UDS to the main Potato
/// process at `POTATO_SOCKET`, and writes responses back to stdout.
async fn run_mcp_server() -> Result<()> {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::UnixStream;

    let pane_id: u64 = std::env::var("POTATO_PANE_ID")
        .map_err(|_| anyhow::anyhow!("POTATO_PANE_ID env var not set"))?
        .parse()
        .map_err(|_| anyhow::anyhow!("POTATO_PANE_ID must be a u64"))?;

    let socket_path = std::env::var("POTATO_SOCKET")
        .map_err(|_| anyhow::anyhow!("POTATO_SOCKET env var not set"))?;

    let stream = UnixStream::connect(&socket_path)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to connect to Potato bridge at {socket_path}: {e}"))?;

    let (uds_read, mut uds_write) = stream.into_split();
    let mut uds_reader = BufReader::new(uds_read);

    let stdin = tokio::io::stdin();
    let mut stdin_reader = BufReader::new(stdin);
    let mut stdout = tokio::io::stdout();

    let mut line = String::new();
    loop {
        line.clear();
        match stdin_reader.read_line(&mut line).await {
            Ok(0) => break, // EOF from Claude — session ending.
            Err(e) => {
                eprintln!("potato mcp-server: stdin error: {e}");
                break;
            }
            Ok(_) => {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }

                // Wrap in bridge protocol and send to main process.
                let bridge_req = serde_json::json!({
                    "pane_id": pane_id,
                    "request": trimmed
                });
                let mut msg = bridge_req.to_string();
                msg.push('\n');

                if let Err(e) = uds_write.write_all(msg.as_bytes()).await {
                    eprintln!("potato mcp-server: UDS write error: {e}");
                    break;
                }

                // Read response from bridge.
                let mut resp_line = String::new();
                match uds_reader.read_line(&mut resp_line).await {
                    Ok(0) => {
                        eprintln!("potato mcp-server: bridge closed connection");
                        break;
                    }
                    Err(e) => {
                        eprintln!("potato mcp-server: UDS read error: {e}");
                        break;
                    }
                    Ok(_) => {
                        // Parse BridgeResponse and extract the inner JSON-RPC response.
                        if let Ok(v) = serde_json::from_str::<serde_json::Value>(resp_line.trim()) {
                            if let Some(response_str) = v["response"].as_str() {
                                let mut out = response_str.to_string();
                                out.push('\n');
                                if let Err(e) = stdout.write_all(out.as_bytes()).await {
                                    eprintln!("potato mcp-server: stdout write error: {e}");
                                    break;
                                }
                                stdout.flush().await.ok();
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(())
}

// ── RAII terminal guard ───────────────────────────────────────────────────────

struct TerminalGuard;

impl TerminalGuard {
    fn enter() -> Result<Self> {
        enable_raw_mode()?;
        execute!(io::stdout(), EnterAlternateScreen, EnableMouseCapture)?;
        Ok(Self)
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), DisableMouseCapture, LeaveAlternateScreen);
    }
}

// ── Agent detection ───────────────────────────────────────────────────────────

/// Detect all available agents on the system.
fn detect_agents() -> Vec<AgentInfo> {
    let mut agents = vec![];

    // Claude Code
    let claude = ClaudeAdapter;
    let claude_path = claude.detect();
    agents.push(AgentInfo {
        name: "Claude Code".to_string(),
        adapter: "claude".to_string(),
        available: claude_path.is_some(),
        binary_path: claude_path,
    });

    // Codex — use real CodexAdapter now
    let codex = CodexAdapter;
    let codex_path = codex.detect();
    agents.push(AgentInfo {
        name: "Codex".to_string(),
        adapter: "codex".to_string(),
        available: codex_path.is_some(),
        binary_path: codex_path,
    });

    // OpenCode (generic fallback)
    let opencode = GenericAdapter::new("opencode");
    let opencode_path = opencode.detect();
    agents.push(AgentInfo {
        name: "OpenCode".to_string(),
        adapter: "opencode".to_string(),
        available: opencode_path.is_some(),
        binary_path: opencode_path,
    });

    agents
}

// ── Async app loop ────────────────────────────────────────────────────────────

async fn run_async(terminal: &mut DefaultTerminal, state: &mut AppState) -> Result<()> {
    let mut event_rx = event_stream();
    let tick_duration = Duration::from_millis(50);

    // Per-turn handle for the current Claude process (None when not processing a turn).
    // Each user message spawns a new process; this is replaced each turn.
    let mut turn_handle: Option<TurnHandle> = None;

    loop {
        // ── Background-thread panic recovery (T-907) ──────────────────────────
        if terminal::panic_hook::take_redraw_flag() {
            terminal.clear()?;
        }

        // ── Render ────────────────────────────────────────────────────────────
        terminal.draw(|frame| {
            let area = frame.area();
            match state.screen {
                AppScreen::Dashboard(_) => {
                    render_dashboard(frame, area, state);
                }
                AppScreen::Session(_) => {
                    render_session(frame, area, state);
                    // Note: render_session takes &mut AppState to resize the PTY.
                }
            }
        })?;

        // ── PTY event drain ───────────────────────────────────────────────────
        if let Some(ref mut handle) = turn_handle {
            // Drain any pending PTY events without blocking.
            loop {
                match handle.event_rx.try_recv() {
                    Ok(event) => apply_pty_event(state, event),
                    Err(_) => break,
                }
            }
            // Check if the turn process has exited — clear the handle when done.
            if handle.exit_rx.borrow().is_some() {
                turn_handle = None;
            }
        }

        // ── Claude session log drain (direct sidebar source-of-truth) ───────
        sync_all_panes(state);

        // ── MCP injection drain ──────────────────────────────────────────────
        // Deliver messages enqueued by the MCP bridge into target pane PTYs.
        drain_inject_requests(state);

        // ── Sync MCP roles → pane titles ─────────────────────────────────────
        sync_mcp_roles_to_panes(state);

        // ── OpenSpec sync ────────────────────────────────────────────────────
        sync_openspec(state);

        // ── Input / message wait ──────────────────────────────────────────────
        let msg = tokio::select! {
            Some(m) = event_rx.recv() => Some(m),
            _ = tokio::time::sleep(tick_duration) => Some(Message::Tick),
        };

        let mut pending_session_resume: Option<String> = None;
        let mut pending_new_session = false;

        if let Some(m) = msg {
            // ── Centralized key dispatch ─────────────────────────────────
            if let Message::Key(ref key) = m {
                match input::handle_key(state, key) {
                    input::KeyAction::Quit => {
                        state.should_quit = true;
                        break;
                    }
                    input::KeyAction::SpawnDashboard => {
                        // Snapshot roles BEFORE spawning — spawn_claude_pane
                        // switches screen to Session, making Dashboard inaccessible.
                        let roles: Vec<crate::app::state::RoleDefinition> =
                            if let AppScreen::Dashboard(ref dash) = state.screen {
                                dash.roles.clone()
                            } else {
                                Vec::new()
                            };
                        let role_count = roles.len().max(1);

                        for _ in 0..role_count.min(2) {
                            match spawn_claude_pane(state, None) {
                                Ok(id) => tracing::info!("Dashboard spawned pane: {}", id),
                                Err(e) => {
                                    tracing::error!("Dashboard spawn failed: {e}");
                                    state.set_error(format!("Spawn failed: {e}"), 100);
                                    break;
                                }
                            }
                        }

                        for (i, role) in roles.iter().enumerate() {
                            if let Some(pane) = state.panes.get_mut(i) {
                                pane.role_name = Some(role.name.clone());
                                if let Some(ref iss) = state.inter_session_state {
                                    if let Ok(mut st) = iss.lock() {
                                        st.set_role(pane.id, crate::mcp::state::PaneRole {
                                            name: role.name.clone(),
                                            description: role.prompt.clone(),
                                        });
                                    }
                                }
                            }
                        }

                        state.tick_count = state.tick_count.wrapping_add(1);
                        continue;
                    }
                    input::KeyAction::ResumeSession(id) => {
                        pending_session_resume = Some(id);
                        // Fall through to update() then deferred resume handler.
                    }
                    input::KeyAction::SpawnAgent => {
                        pending_new_session = true;
                        // Fall through to deferred spawn handler.
                    }
                    input::KeyAction::ClosePane => {
                        if let Some(closed) = state.panes.close_active() {
                            if let Some(ref iss) = state.inter_session_state {
                                if let Ok(mut st) = iss.lock() {
                                    st.unregister_pane(closed.id);
                                }
                            }
                        }

                        turn_handle = None;

                        if state.panes.is_empty() {
                            if let Ok(cwd) = std::env::current_dir() {
                                let _ = crate::mcp::config_writer::remove_mcp_config(&cwd);
                            }
                        }

                        if state.panes.is_empty() {
                            state.screen = AppScreen::Dashboard(DashboardState {
                                available_agents: detect_agents(),
                                ..DashboardState::default()
                            });
                        }
                        continue;
                    }
                    input::KeyAction::Broadcast(text) => {
                        let n_panes = state.panes.len();
                        let mut any_written = false;
                        for i in 0..n_panes {
                            if let Some(pane) = state.panes.get_mut(i) {
                                if let Some(ref mut pty) = pane.pty {
                                    if !pty.child_exited() {
                                        if let Err(e) = pty.write_input(text.as_bytes()) {
                                            tracing::warn!("Broadcast text to pane {i}: {e}");
                                        } else {
                                            any_written = true;
                                            if let Ok(mut pending) = PENDING_ENTERS.lock() {
                                                pending.push(crate::mcp::injection::PendingEnter {
                                                    pane_index: i,
                                                    written_at_tick: state.tick_count,
                                                    delay_ticks: crate::mcp::injection::ENTER_DELAY_TICKS,
                                                });
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        if !any_written {
                            tracing::warn!("No panes to broadcast to");
                        }
                        continue;
                    }
                    input::KeyAction::Handled => {
                        continue;
                    }
                    input::KeyAction::Unhandled => {
                        // Fall through to update().
                    }
                }
                // Old key handling removed — now in src/input/ module.
            } // end if let Message::Key


            if let Message::Mouse(ref mouse) = m {
                if matches!(state.screen, AppScreen::Session(_)) {
                    let current_focus = state
                        .session()
                        .map(|s| s.cockpit_focus)
                        .unwrap_or(CockpitFocus::Input);

                    if current_focus == CockpitFocus::Terminal {
                        let scroll_sess = if !state.panes.is_empty() {
                            state.panes.active_pane_mut().map(|p| &mut p.session)
                        } else {
                            state.session_mut()
                        };
                        if let Some(session) = scroll_sess {
                            match mouse.kind {
                                MouseEventKind::ScrollUp => {
                                    session.scroll_terminal_up(3);
                                    continue;
                                }
                                MouseEventKind::ScrollDown => {
                                    session.scroll_terminal_down(3);
                                    continue;
                                }
                                _ => {}
                            }
                        }
                    }

                    // Git panel mouse scroll.
                    if current_focus == CockpitFocus::Git {
                        if let AppScreen::Session(ref mut session) = state.screen {
                            match mouse.kind {
                                MouseEventKind::ScrollUp => {
                                    session.git_scroll = session.git_scroll.saturating_sub(3);
                                    continue;
                                }
                                MouseEventKind::ScrollDown => {
                                    session.git_scroll = session.git_scroll.saturating_add(3);
                                    continue;
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }

            // Standard update/action dispatch (skip if we're about to resume).
            if pending_session_resume.is_none() {
                let action = update(state, m);
                use app::action::Action;
                if matches!(action, Action::Quit) {
                    break;
                }
            }
        }

        // ── Resume a historical session (deferred from key handler) ───────
        if let Some(resume_id) = pending_session_resume.take() {
            let prev_selected = state.session().map(|s| s.selected_session).unwrap_or(0);
            match spawn_claude_pane(state, Some(&resume_id)) {
                Ok(_) => {
                    // Restore rail selection.
                    if let Some(ref mut session) = state.session_mut() {
                        session.selected_session = prev_selected;
                    }
                    tracing::info!("Resumed session in pane: {}", resume_id);
                }
                Err(e) => state.set_error(e, 100),
            }
        }

        // ── Spawn a new agent session (deferred from Agents Enter) ──────────
        if pending_new_session {
            let agent_rows = crate::ui::overlays::agent_picker::build_agent_rows();
            let selected_idx = state.session().map(|s| s.selected_agent).unwrap_or(0);
            let adapter_name = agent_rows
                .get(selected_idx)
                .map(|r| r.adapter_name.as_str())
                .unwrap_or("claude");
            match spawn_agent_pane(state, adapter_name, None) {
                Ok(id) => tracing::info!("New {} pane spawned: {}", adapter_name, id),
                Err(e) => state.set_error(e, 100),
            }
        }

        // ── Detect dead panes (Claude exited) and close them ──────────────
        {
            let mut dead_indices: Vec<usize> = Vec::new();
            for i in 0..state.panes.len() {
                if let Some(pane) = state.panes.get(i) {
                    if let Some(ref pty) = pane.pty {
                        // Check if the PTY child has exited by trying to read.
                        // A dead PTY will have its reader return immediately with empty data
                        // or error. We use the `is_alive` check on the child process.
                        if pty.child_exited() {
                            dead_indices.push(i);
                        }
                    }
                }
            }
            // Close dead panes (iterate in reverse to preserve indices).
            let had_panes = !dead_indices.is_empty();
            for i in dead_indices.into_iter().rev() {
                tracing::info!("Pane {} PTY exited, closing", i);
                if let Some(closed) = state.panes.close(i) {
                    if let Some(ref iss) = state.inter_session_state {
                        if let Ok(mut st) = iss.lock() {
                            st.unregister_pane(closed.id);
                        }
                    }
                }
            }
            // Only bounce to dashboard if we just closed the last pane.
            if had_panes && state.panes.is_empty() {
                // Clean up .mcp.json when all panes are gone.
                if let Ok(cwd) = std::env::current_dir() {
                    let _ = crate::mcp::config_writer::remove_mcp_config(&cwd);
                }
            }
            if had_panes && state.panes.is_empty() && matches!(state.screen, AppScreen::Session(_)) {
                tracing::info!("All panes closed, returning to dashboard");
                state.screen = AppScreen::Dashboard(DashboardState {
                    available_agents: detect_agents(),
                    ..DashboardState::default()
                });
            }
        }

        // Tick counter (drives animations).
        state.tick_count = state.tick_count.wrapping_add(1);
        let tc = state.tick_count;
        if let Some(session) = state.session_mut() {
            session.tick_count = tc;
        }

        // Periodic left-rail refresh (every 30 s, even without JSONL changes).
        {
            let now = unix_now();
            if now - state.last_rail_refresh >= 30 {
                if let Some(store) = state.store.clone() {
                    refresh_rail(state, &store);
                }
            }
        }

        // Periodic git refresh (~30 s at 250ms tick = every 120 ticks).
        state.git_refresh_ticks += 1;
        if state.git_refresh_ticks >= 120 {
            state.git_refresh_ticks = 0;
            state.git_snapshot = git::GitSnapshot::capture();
        }

        if state.should_quit {
            break;
        }
    }

    Ok(())
}

/// Spawn a Claude PTY session into the pane manager.
///
/// If `resume_id` is `Some`, resumes an existing session via `--resume <id>`.
/// Otherwise creates a new session with `--session-id <uuid>`.
///
/// Returns the session id on success.
fn spawn_claude_pane(
    state: &mut AppState,
    resume_id: Option<&str>,
) -> Result<String, String> {
    let binary = which::which("claude").map_err(|_| "Claude binary not found".to_string())?;

    if !state.panes.can_open() {
        return Err("Maximum panes already open".to_string());
    }

    let (term_cols, term_rows) = crossterm::terminal::size().unwrap_or((120, 40));
    // Split center among panes.
    let n_panes = state.panes.len() + 1; // after we open one
    let center_cols = (term_cols as u32 * 3 / 4).saturating_sub(2);
    let pty_cols = (center_cols / n_panes as u32).max(20) as u16;
    let pty_rows = term_rows.saturating_sub(10);

    let launch_cwd = std::env::current_dir().ok();

    let (session_id, session_args_owned): (String, Vec<String>) = if let Some(rid) = resume_id {
        (rid.to_string(), vec![
            "--resume".into(), rid.into(),
            "--dangerously-skip-permissions".into(),
        ])
    } else {
        let id = uuid::Uuid::new_v4().to_string();
        let args = vec![
            "--session-id".into(), id.clone(),
            "--dangerously-skip-permissions".into(),
        ];
        (id, args)
    };

    let session_args_refs: Vec<&str> = session_args_owned.iter().map(|s| s.as_str()).collect();

    // ── MCP env vars ──────────────────────────────────────────────────────────
    // Set POTATO_PANE_ID and POTATO_SOCKET on every pane's PTY process.
    // The MCP server inherits these from its parent (Claude PTY), so
    // .mcp.json only needs a single shared "potato" entry.
    let pane_index_after_open = state.panes.len(); // 0-based index the new pane will occupy
    let mut pane_env: Vec<(String, String)> = Vec::new();
    if let Some(ref sock) = state.mcp_socket_path.clone() {
        // Always provide the socket path and the future pane id to every pane.
        // The pane id matches `PaneManager::next_id` — we can read it from the manager.
        let pane_id: u64 = state.panes.next_id();
        pane_env.push(("POTATO_PANE_ID".into(), pane_id.to_string()));
        pane_env.push(("POTATO_SOCKET".into(), sock.to_string_lossy().into_owned()));
    }

    let real_pty = if pane_env.is_empty() {
        crate::pty::RealPty::spawn_in(
            binary.to_str().unwrap_or("claude"),
            &session_args_refs,
            pty_cols.max(20),
            pty_rows.max(5),
            launch_cwd.as_deref(),
        )
    } else {
        crate::pty::RealPty::spawn_with_env(
            binary.to_str().unwrap_or("claude"),
            &session_args_refs,
            pty_cols.max(20),
            pty_rows.max(5),
            launch_cwd.as_deref(),
            &pane_env,
        )
    }
    .map_err(|e| format!("PTY spawn failed: {e}"))?;

    // Open pane in manager.
    let pane = state
        .panes
        .open(&session_id, "claude")
        .ok_or_else(|| "Failed to open pane".to_string())?;

    let pane_id = pane.id;
    let _dirty_rx = real_pty.dirty_tx.subscribe();
    pane.pty = Some(real_pty);
    pane.session.status = crate::app::state::AgentStatus::Idle;
    pane.session.claude_session_id = Some(session_id.clone());

    // Register pane with inter-session state for partner resolution.
    if let Some(ref iss) = state.inter_session_state {
        if let Ok(mut st) = iss.lock() {
            st.register_pane(pane_id);
        }
    }

    // Set up JSONL log tracker.
    if let Some(home) = dirs::home_dir() {
        let cwd = launch_cwd.as_deref().unwrap_or(&home);
        let path = crate::claude_log::session_log_path(&home, cwd, &session_id);
        tracing::info!("Claude session log: {}", path.display());
        pane.log = Some(crate::claude_log::ClaudeSessionLogTracker::new(path));
    }

    // Ensure we're on the session screen.
    if !matches!(state.screen, AppScreen::Session(_)) {
        state.enter_session(&session_id, "claude");
    }
    if let Some(ref mut session) = state.session_mut() {
        session.status = crate::app::state::AgentStatus::Idle;
        session.claude_session_id = Some(session_id.clone());
    }

    // Persist to SQLite.
    if let Some(ref store) = state.store.clone() {
        let project_dir = launch_cwd
            .as_deref()
            .map(crate::claude_log::project_dir_name)
            .unwrap_or_default();
        let cwd_str = launch_cwd.as_deref().and_then(|p| p.to_str()).map(str::to_string);
        let now = unix_now();
        if let Err(e) = store.upsert_session(
            &session_id,
            &project_dir,
            "claude",
            None,
            "",
            cwd_str.as_deref(),
            0, 0, 0,
            now, now,
        ) {
            tracing::warn!("Failed to create session row: {e}");
        }
        refresh_rail(state, store);
    }

    // ── Write .mcp.json ────────────────────────────────────────────────────────
    // Always keep .mcp.json in sync with current panes so Claude discovers
    // Potato MCP tools. Written on every pane spawn (not just the 2nd) so
    // that pane 0 has the config available if a 2nd pane is opened later
    // and Claude re-reads it on the next conversation turn.
    let wrote_mcp = if state.panes.len() >= 1 {
        if let Some(ref _sock) = state.mcp_socket_path.clone() {
            if let Some(ref cwd) = launch_cwd {
                // Single shared "potato" MCP entry. Each Claude PTY inherits
                // POTATO_PANE_ID + POTATO_SOCKET from its env, so the spawned
                // MCP server process knows which pane it belongs to.
                if let Err(e) = crate::mcp::config_writer::write_mcp_config(cwd, &[], "") {
                    tracing::warn!("Failed to write .mcp.json: {e}");
                    false
                } else {
                    tracing::info!("Wrote .mcp.json (shared potato MCP entry)");
                    true
                }
            } else { false }
        } else { false }
    } else { false };

    // ── Auto-fill broadcast bar with collaboration prompt ───────────────────
    // When the second pane spawns, pre-fill the input (broadcast) bar with
    // a collaboration context message. User sees it, can edit, hits Enter
    // to broadcast to both agents simultaneously.
    if wrote_mcp && state.panes.len() == 2 {
        let collab_prompt = build_collaboration_prompt(state);
        if let AppScreen::Session(ref mut session) = state.screen {
            session.input_buffer = collab_prompt.clone();
            session.input_cursor = collab_prompt.len();
        }
    }

    tracing::info!("Opened Claude pane for session: {}", session_id);
    Ok(session_id)
}

/// Build a collaboration prompt to pre-fill the broadcast bar.
///
/// This is what the user sees and can edit before hitting Enter to send
/// to all agents simultaneously.
fn build_collaboration_prompt(state: &AppState) -> String {
    let summaries: Vec<PaneSummary> = (0..state.panes.len())
        .filter_map(|i| {
            state.panes.get(i).map(|p| PaneSummary {
                id: p.id,
                agent_name: p.session.agent_name.clone(),
                role_name: p.role_name.clone(),
            })
        })
        .collect();
    build_collaboration_prompt_from_panes(&summaries)
}

/// Spawn a PTY session for any supported agent adapter.
///
/// Delegates to `spawn_claude_pane` for the `"claude"` adapter.
/// For `"codex"`, spawns Codex in interactive PTY mode.
/// For anything else, uses a generic PTY spawn.
///
/// Returns the session id on success.
fn spawn_agent_pane(
    state: &mut AppState,
    adapter: &str,
    resume_id: Option<&str>,
) -> Result<String, String> {
    match adapter {
        "claude" => spawn_claude_pane(state, resume_id),
        "codex" => {
            use crate::adapters::codex::CodexAdapter;
            use crate::adapters::AgentAdapter;

            let codex = CodexAdapter;
            let binary = codex
                .detect()
                .ok_or_else(|| "Codex binary not found".to_string())?;

            if !state.panes.can_open() {
                return Err("Maximum panes already open".to_string());
            }

            let (term_cols, term_rows) = crossterm::terminal::size().unwrap_or((120, 40));
            let n_panes = state.panes.len() + 1;
            let center_cols = (term_cols as u32 * 3 / 4).saturating_sub(2);
            let pty_cols = (center_cols / n_panes as u32).max(20) as u16;
            let pty_rows = term_rows.saturating_sub(10);

            let launch_cwd = std::env::current_dir().ok();

            let (session_id, spawn_args_owned): (String, Vec<String>) = if let Some(rid) = resume_id {
                (rid.to_string(), vec!["resume".into(), rid.into()])
            } else {
                let id = uuid::Uuid::new_v4().to_string();
                (id, vec![])
            };

            let spawn_args_refs: Vec<&str> = spawn_args_owned.iter().map(|s| s.as_str()).collect();

            let real_pty = crate::pty::RealPty::spawn_in(
                binary.to_str().unwrap_or("codex"),
                &spawn_args_refs,
                pty_cols.max(20),
                pty_rows.max(5),
                launch_cwd.as_deref(),
            )
            .map_err(|e| format!("PTY spawn failed: {e}"))?;

            let pane = state
                .panes
                .open(&session_id, "codex")
                .ok_or_else(|| "Failed to open pane".to_string())?;

            pane.pty = Some(real_pty);
            pane.session.status = crate::app::state::AgentStatus::Idle;
            pane.session.claude_session_id = Some(session_id.clone());

            // Set up Codex JSONL log tracker.
            if let Some(home) = dirs::home_dir() {
                if let Some(path) = crate::codex_log::find_session_log(&home, &session_id) {
                    tracing::info!("Codex session log: {}", path.display());
                }
                // Note: Codex session file is created after first prompt, so we
                // can't set up the tracker here. It will be discovered on next poll.
            }

            // Transition to session screen.
            if !matches!(state.screen, AppScreen::Session(_)) {
                state.enter_session(&session_id, "codex");
            }
            if let Some(ref mut session) = state.session_mut() {
                session.status = crate::app::state::AgentStatus::Idle;
                session.claude_session_id = Some(session_id.clone());
            }

            // Persist to SQLite.
            if let Some(ref store) = state.store.clone() {
                let project_dir = launch_cwd
                    .as_deref()
                    .map(crate::claude_log::project_dir_name)
                    .unwrap_or_default();
                let now = crate::session::unix_now();
                if let Err(e) = store.upsert_session(
                    &session_id,
                    &project_dir,
                    "codex",
                    None,
                    "",
                    launch_cwd.as_deref().and_then(|p| p.to_str()),
                    0, 0, 0,
                    now, now,
                ) {
                    tracing::warn!("Failed to create codex session row: {e}");
                }
                refresh_rail(state, store);
            }

            tracing::info!("Opened Codex pane for session: {}", session_id);
            Ok(session_id)
        }
        other => {
            // Generic adapter — spawn the binary directly as a PTY.
            use crate::adapters::generic::GenericAdapter;
            use crate::adapters::AgentAdapter;

            let generic = GenericAdapter::new(other);
            let binary = generic
                .detect()
                .ok_or_else(|| format!("{other} binary not found"))?;

            if !state.panes.can_open() {
                return Err("Maximum panes already open".to_string());
            }

            let (term_cols, term_rows) = crossterm::terminal::size().unwrap_or((120, 40));
            let n_panes = state.panes.len() + 1;
            let center_cols = (term_cols as u32 * 3 / 4).saturating_sub(2);
            let pty_cols = (center_cols / n_panes as u32).max(20) as u16;
            let pty_rows = term_rows.saturating_sub(10);

            let launch_cwd = std::env::current_dir().ok();
            let session_id = uuid::Uuid::new_v4().to_string();

            let real_pty = crate::pty::RealPty::spawn_in(
                binary.to_str().unwrap_or(other),
                &[],
                pty_cols.max(20),
                pty_rows.max(5),
                launch_cwd.as_deref(),
            )
            .map_err(|e| format!("PTY spawn failed: {e}"))?;

            let pane = state
                .panes
                .open(&session_id, other)
                .ok_or_else(|| "Failed to open pane".to_string())?;

            pane.pty = Some(real_pty);
            pane.session.status = crate::app::state::AgentStatus::Idle;
            pane.session.claude_session_id = Some(session_id.clone());

            if !matches!(state.screen, AppScreen::Session(_)) {
                state.enter_session(&session_id, other);
            }

            tracing::info!("Opened {} pane for session: {}", other, session_id);
            Ok(session_id)
        }
    }
}

/// Sync all pane JSONL trackers and update sidebar metrics.
/// Drain pending injection requests from the MCP bridge and write formatted
/// notifications into target pane PTYs so agents actually "see" messages.
/// Pending `\r` submissions awaiting their delay.
static PENDING_ENTERS: std::sync::Mutex<Vec<crate::mcp::injection::PendingEnter>> =
    std::sync::Mutex::new(Vec::new());

fn drain_inject_requests(state: &mut AppState) {
    let current_tick = state.tick_count;

    // ── Phase 1: Drain new requests → write text (no \r yet) ─────────────
    if let Some(ref mut rx) = state.inject_rx {
        while let Ok(req) = rx.try_recv() {
            let notification = crate::mcp::injection::format_notification(
                req.from_pane,
                req.from_role.as_deref(),
                &req.content,
            );

            let target_index = (0..state.panes.len())
                .find(|&i| state.panes.get(i).map(|p| p.id) == Some(req.to_pane));

            match target_index {
                Some(idx) => {
                    match crate::mcp::injection::inject_into_pane(
                        &mut state.panes,
                        idx,
                        &notification,
                    ) {
                        Ok(true) => {
                            tracing::info!(
                                "Injected text from pane {} to pane {} (Enter pending)",
                                req.from_pane, req.to_pane
                            );
                            if let Ok(mut pending) = PENDING_ENTERS.lock() {
                                pending.push(crate::mcp::injection::PendingEnter {
                                    pane_index: idx,
                                    written_at_tick: current_tick,
                                    delay_ticks: crate::mcp::injection::ENTER_DELAY_TICKS,
                                });
                            }
                        }
                        Ok(false) => {
                            tracing::warn!(
                                "Skipped injection to pane {} (approval pending or no PTY)",
                                req.to_pane
                            );
                        }
                        Err(e) => {
                            tracing::warn!("Injection to pane {} failed: {e}", req.to_pane);
                        }
                    }
                }
                None => {
                    tracing::warn!("Inject target pane {} not found", req.to_pane);
                }
            }
        }
    }

    // ── Phase 2: Send `\r` for any pending enters whose delay has elapsed ────
    if let Ok(mut pending) = PENDING_ENTERS.lock() {
        pending.retain(|p| {
            if current_tick.wrapping_sub(p.written_at_tick) >= p.delay_ticks {
                // Time to send Enter.
                if let Some(pane) = state.panes.get_mut(p.pane_index) {
                    if let Some(ref mut pty) = pane.pty {
                        if !pty.child_exited() {
                            if let Err(e) = pty.write_input(b"\r") {
                                tracing::warn!("Failed to send Enter to pane: {e}");
                            } else {
                                tracing::info!("Sent deferred Enter to pane index {}", p.pane_index);
                            }
                        }
                    }
                }
                false // remove from pending
            } else {
                true // keep waiting
            }
        });
    }
}

/// Sync role names from MCP InterSessionState into pane.role_name so the UI
/// reflects roles claimed via MCP tools (not just the /role slash command).
fn sync_mcp_roles_to_panes(state: &mut AppState) {
    let roles: Vec<(u64, crate::mcp::state::PaneRole)> = match state.inter_session_state {
        Some(ref inter) => match inter.lock() {
            Ok(st) => st.list_roles().into_iter().map(|(id, r)| (id, r.clone())).collect(),
            Err(_) => return,
        },
        None => return,
    };

    for (pane_id, role) in roles {
        if let Some(idx) = state.panes.find_by_pane_id(pane_id) {
            if let Some(pane) = state.panes.get_mut(idx) {
                if pane.role_name.as_deref() != Some(&role.name) {
                    pane.role_name = Some(role.name.clone());
                    if !role.description.is_empty() {
                        pane.role_description = Some(role.description.clone());
                    }
                }
            }
        }
    }
}

/// Sync OpenSpec: poll watcher for file changes and refresh snapshot in InterSessionState.
fn sync_openspec(state: &mut AppState) {
    // 1. Poll watcher for file-change notifications (non-blocking).
    let mut changed = false;
    if let Some(ref mut openspec) = state.openspec {
        while openspec.rx.try_recv().is_ok() {
            changed = true;
        }
    }

    // 2. Refresh the OpenSpec snapshot in InterSessionState on change.
    if changed {
        if let (Some(openspec), Some(iss)) = (&state.openspec, &state.inter_session_state) {
            let tasks = openspec.open_tasks();
            let snapshots: Vec<mcp::state::OpenSpecTaskSnapshot> = tasks.iter().map(|t| {
                mcp::state::OpenSpecTaskSnapshot {
                    id: t.id.clone(),
                    title: t.title.clone(),
                    status: t.status.to_string(),
                    phase: t.phase.clone(),
                    severity: t.severity.clone(),
                }
            }).collect();
            if let Ok(mut st) = iss.lock() {
                st.openspec_tasks = snapshots;
            }
            tracing::debug!("OpenSpec task snapshot refreshed ({} open tasks)", tasks.len());
        }
    }
}

fn sync_all_panes(state: &mut AppState) {
    let store = state.store.clone();
    let now = unix_now();
    let mut any_title_changed = false;

    for i in 0..state.panes.len() {
        let pane = match state.panes.get_mut(i) {
            Some(p) => p,
            None => continue,
        };
        let tracker = match pane.log.as_mut() {
            Some(t) => t,
            None => continue,
        };
        let Ok(changed) = tracker.poll() else { continue; };
        if !changed {
            continue;
        }

        let snapshot = tracker.snapshot();

        // Update live sidebar metrics on the pane's session.
        pane.session.metrics.input_tokens = snapshot.usage.input_tokens;
        pane.session.metrics.output_tokens = snapshot.usage.output_tokens;
        pane.session.tokens_used = snapshot.usage.total_tokens();

        // Persist to SQLite.
        let session_id = match &pane.session.claude_session_id {
            Some(id) if !id.is_empty() => id.clone(),
            _ => continue,
        };

        if let Some(ref store) = store {
            let project_dir = if let Some(home) = dirs::home_dir() {
                let cwd = std::env::current_dir().ok().unwrap_or_else(|| home.clone());
                crate::claude_log::project_dir_name(&cwd)
            } else {
                String::new()
            };

            let title = if !snapshot.title.is_empty() {
                snapshot.title.clone()
            } else {
                state.rail_sessions.iter()
                    .find(|s| s.id == session_id)
                    .map(|s| s.title.clone())
                    .unwrap_or_default()
            };

            let old_title = state.rail_sessions.iter()
                .find(|s| s.id == session_id)
                .map(|s| s.title.clone())
                .unwrap_or_default();

            if title != old_title {
                any_title_changed = true;
            }

            if let Err(e) = store.upsert_session(
                &session_id,
                &project_dir,
                "claude",
                snapshot.model.as_deref(),
                &title,
                std::env::current_dir().ok().as_deref().and_then(|p| p.to_str()),
                snapshot.usage.input_tokens,
                snapshot.usage.output_tokens,
                snapshot.turns,
                now, now,
            ) {
                tracing::warn!("sync pane {}: upsert_session failed: {e}", i);
            }
        }
    }

    if any_title_changed {
        if let Some(ref store) = state.store.clone() {
            refresh_rail(state, store);
        }
    }
}

/// Re-query the session list from SQLite and cache it in `AppState`.
fn refresh_rail(state: &mut AppState, store: &SessionStore) {
    match store.list_sessions() {
        Ok(sessions) => {
            state.rail_sessions = sessions;
            state.last_rail_refresh = unix_now();
        }
        Err(e) => {
            tracing::warn!("refresh_rail: list_sessions failed: {e}");
        }
    }
}

fn apply_pty_event(state: &mut AppState, event: crate::events::AgentEvent) {
    use crate::events::AgentEvent;

    // Handle side-effectful variants that cannot live in the pure reducer.
    match &event {
        AgentEvent::Warning { message } => {
            tracing::warn!("{}", message);
            return;
        }
        AgentEvent::Raw { payload } => {
            // Raw lines are appended as assistant text; delegate to reducer.
            let text_event = AgentEvent::TextDelta { text: {
                let mut s = payload.clone();
                s.push('\n');
                s
            }};
            if let Some(session) = state.session_mut() {
                app::session_reducer::apply_event(session, text_event, chrono::Utc::now());
            }
            return;
        }
        _ => {}
    }

    // ── Panel routing (Phase-3) ───────────────────────────────────────────────
    match &event {
        AgentEvent::ToolStart { id, name, input } => {
            let record = app::state::ToolCallRecord {
                id: id.clone(),
                name: name.clone(),
                input: input.clone(),
                output: None,
                started_at: chrono::Utc::now(),
                duration_ms: None,
                success: None,
            };
            state.tool_output_panel.add_entry(&record);
        }
        AgentEvent::ToolDone { id, output, duration_ms, success } => {
            state.tool_output_panel.update_entry(
                id,
                Some(output.clone()),
                Some(*duration_ms),
                Some(*success),
            );
        }
        AgentEvent::ToolError { id, error } => {
            state.tool_output_panel.update_entry(id, Some(error.clone()), Some(0), Some(false));
        }
        _ => {}
    }

    if let Some(session) = state.session_mut() {
        app::session_reducer::apply_event(session, event, chrono::Utc::now());
    }
}

// ── Entry point ───────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<()> {
    // Initialise file-based logging first — before anything else.
    // All tracing output goes to ~/.potato/potato.log, never to the terminal.
    if let Err(e) = log::init_file_logging() {
        eprintln!("Warning: could not initialise file logging: {e}");
    }

    // Redirect stderr (fd 2) to the log file so that eprintln!, panic
    // output, and library debug spew never corrupt the ratatui surface.
    // This must happen after init_file_logging (which sets up tracing)
    // but before we enter the TUI.
    if let Err(e) = log::redirect_stderr(&log::log_path()) {
        tracing::warn!("could not redirect stderr: {e}");
    }

    let cli = Cli::parse();

    // ── Handle subcommands before TUI setup ───────────────────────────────────
    if let Some(PotatoCommand::McpServer) = cli.command {
        return run_mcp_server().await;
    }

    install_panic_hook();

    // Load configuration.
    let cfg = load_config(cli.config.as_deref())?;

    // Initialise session store.
    let db_path = config::expand_tilde(&cfg.db_path);
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    let store = Arc::new(SessionStore::open(&db_path.to_string_lossy())?);

    // Scan ~/.claude/projects/ once at startup and upsert all known sessions.
    if let Some(home) = dirs::home_dir() {
        discover_historical_sessions(&home, &store);
    }

    // Build initial state with detected agents.
    let agents = detect_agents();
    // The --model flag is stored in AppState directly (Claude picks its own model;
    // this is only used for display purposes in the status bar).
    let model = cli.model.unwrap_or_else(|| cfg.default_agent.clone());

    // Snapshot filesystem paths once (not every render frame).
    let path_snapshots = {
        use crate::app::state::PathSnapshots;
        PathSnapshots {
            cwd: std::env::current_dir()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|_| "unknown".to_string()),
            potato_exists: std::path::Path::new(".potato").exists(),
            openspec_exists: std::path::Path::new("openspec/changes").exists(),
            mcp_json_exists: std::path::Path::new(".mcp.json").exists(),
        }
    };

    // Pre-load session list for the left rail.
    let initial_sessions = store.list_sessions().unwrap_or_default();

    // Capture git repo state once at startup.
    let git_snapshot = git::GitSnapshot::capture();

    // Start MCP bridge (UDS listener for inter-session communication).
    // Open project-scoped persistent store at `<cwd>/.potato/state.db`.
    let project_store: Option<Arc<std::sync::Mutex<mcp::project_store::ProjectStore>>> = match std::env::current_dir() {
        Ok(cwd) => match mcp::project_store::ProjectStore::open(&cwd) {
            Ok(ps) => {
                tracing::info!("Project store opened at {}/.potato/state.db", cwd.display());
                Some(Arc::new(std::sync::Mutex::new(ps)))
            }
            Err(e) => {
                tracing::warn!("Failed to open project store: {e}");
                None
            }
        },
        Err(e) => {
            tracing::warn!("Could not determine cwd for project store: {e}");
            None
        }
    };
    let inter_state = Arc::new(std::sync::Mutex::new(
        match project_store {
            Some(ref ps) => mcp::state::InterSessionState::with_store(Arc::clone(ps)),
            None => mcp::state::InterSessionState::new(),
        }
    ));
    let (inject_tx, inject_rx) = tokio::sync::mpsc::unbounded_channel();
    let (_mcp_bridge, mcp_socket_path) = mcp::bridge::McpBridge::start(Arc::clone(&inter_state), inject_tx)?;

    // Initialize OpenSpec watcher if `openspec/changes/` exists.
    let openspec_watcher = std::env::current_dir()
        .ok()
        .and_then(|cwd| {
            tracing::info!("Looking for OpenSpec at {}/openspec/changes/", cwd.display());
            let w = openspec::OpenSpecWatcher::new(&cwd);
            if w.is_some() {
                tracing::info!("OpenSpec watcher active");
            } else {
                tracing::warn!("No OpenSpec changes found in {}", cwd.display());
            }
            w
        });

    // Load persisted role definitions from `.potato/roles.toml`.
    let project_roles = std::env::current_dir()
        .map(|cwd| roles::load_roles(&cwd))
        .unwrap_or_default();

    let mut state = AppState {
        model,
        config: cfg,
        screen: AppScreen::Dashboard(DashboardState {
            available_agents: agents,
            roles: project_roles,
            path_snapshots,
            ..DashboardState::default()
        }),
        store: Some(store),
        rail_sessions: initial_sessions,
        git_snapshot,
        git_refresh_ticks: 0,
        last_rail_refresh: unix_now(),
        mcp_socket_path: Some(mcp_socket_path),
        inter_session_state: Some(inter_state),
        inject_rx: Some(inject_rx),
        openspec: openspec_watcher,
        ..AppState::default()
    };

    // Seed OpenSpec snapshot into InterSessionState so agents can read it immediately.
    if let (Some(openspec), Some(iss)) = (&state.openspec, &state.inter_session_state) {
        let tasks = openspec.open_tasks();
        let snapshots: Vec<mcp::state::OpenSpecTaskSnapshot> = tasks.iter().map(|t| {
            mcp::state::OpenSpecTaskSnapshot {
                id: t.id.clone(),
                title: t.title.clone(),
                status: t.status.to_string(),
                phase: t.phase.clone(),
                severity: t.severity.clone(),
            }
        }).collect();
        if let Ok(mut st) = iss.lock() {
            st.openspec_tasks = snapshots;
        }
    }

    // Enter TUI.
    let _guard = TerminalGuard::enter()?;
    let mut terminal = ratatui::init();

    let result = run_async(&mut terminal, &mut state).await;

    ratatui::restore();
    result
}

// ── Pure helper for collaboration prompt (testable) ───────────────────────

/// Pane summary for prompt building — decoupled from AppState.
struct PaneSummary {
    id: u64,
    agent_name: String,
    role_name: Option<String>,
}

/// Build collaboration broadcast text from pane summaries alone.
fn build_collaboration_prompt_from_panes(panes: &[PaneSummary]) -> String {
    let pane_labels: Vec<String> = panes
        .iter()
        .map(|p| {
            if let Some(ref r) = p.role_name {
                format!("Pane {} ({}, role: {})", p.id, p.agent_name, r)
            } else {
                format!("Pane {} ({})", p.id, p.agent_name)
            }
        })
        .collect();

    let has_roles = panes.iter().any(|p| p.role_name.is_some());

    let role_instructions = if has_roles {
        "Your role has already been assigned and claimed for you — call potato_get_role \
         to confirm. Do NOT pick a different role. \
         Use your assigned role name exactly if you need to re-claim."
    } else {
        "IMPORTANT: Before picking a role, call potato_get_role to see what's taken. \
         Then call potato_claim_role with a DIFFERENT role than your partner."
    };

    format!(
        "You are in a multi-agent collaboration managed by Potato. \
         Active panes: {}. \
         You have Potato MCP tools available: \
         potato_claim_role, potato_get_role, \
         potato_send_message, potato_get_messages, potato_get_partner_status, \
         potato_shared_context, potato_claim_task, potato_release_task. \
         {} \
         When given work, use potato_claim_task with the OpenSpec ticket ID (e.g. T-810) \
         to register what you're working on. Release tasks when done. \
         After confirming your role, WAIT for the user to give you a task. \
         Do NOT start working on anything until the user tells you what to do. \
         Introduce yourself briefly and stand by.",
        pane_labels.join(", "),
        role_instructions,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── build_collaboration_prompt_from_panes ────────────────────────────

    #[test]
    fn collab_prompt_includes_role_names_in_pane_labels() {
        let panes = vec![
            PaneSummary { id: 0, agent_name: "claude".into(), role_name: Some("Planner".into()) },
            PaneSummary { id: 1, agent_name: "claude".into(), role_name: Some("Worker".into()) },
        ];
        let prompt = build_collaboration_prompt_from_panes(&panes);
        assert!(prompt.contains("Pane 0 (claude, role: Planner)"), "missing pane 0 role label");
        assert!(prompt.contains("Pane 1 (claude, role: Worker)"), "missing pane 1 role label");
    }

    #[test]
    fn collab_prompt_with_roles_tells_agents_not_to_pick() {
        let panes = vec![
            PaneSummary { id: 0, agent_name: "claude".into(), role_name: Some("Planner".into()) },
            PaneSummary { id: 1, agent_name: "claude".into(), role_name: Some("Worker".into()) },
        ];
        let prompt = build_collaboration_prompt_from_panes(&panes);
        assert!(prompt.contains("already been assigned"), "should tell agents role is pre-assigned");
        assert!(prompt.contains("Do NOT pick a different role"), "should forbid self-selection");
        assert!(!prompt.contains("Before picking a role"), "should NOT include self-selection instructions");
    }

    #[test]
    fn collab_prompt_without_roles_allows_self_selection() {
        let panes = vec![
            PaneSummary { id: 0, agent_name: "claude".into(), role_name: None },
            PaneSummary { id: 1, agent_name: "claude".into(), role_name: None },
        ];
        let prompt = build_collaboration_prompt_from_panes(&panes);
        assert!(prompt.contains("Before picking a role"), "should include self-selection instructions");
        assert!(!prompt.contains("already been assigned"), "should NOT say pre-assigned");
    }

    #[test]
    fn collab_prompt_no_role_labels_without_roles() {
        let panes = vec![
            PaneSummary { id: 0, agent_name: "claude".into(), role_name: None },
            PaneSummary { id: 1, agent_name: "codex".into(), role_name: None },
        ];
        let prompt = build_collaboration_prompt_from_panes(&panes);
        assert!(prompt.contains("Pane 0 (claude)"), "should show agent only");
        assert!(prompt.contains("Pane 1 (codex)"), "should show agent only");
        assert!(!prompt.contains("role:"), "no role: labels expected");
    }

    #[test]
    fn collab_prompt_mixed_roles_treats_as_has_roles() {
        // If even one pane has a role, use pre-assigned instructions
        let panes = vec![
            PaneSummary { id: 0, agent_name: "claude".into(), role_name: Some("Planner".into()) },
            PaneSummary { id: 1, agent_name: "claude".into(), role_name: None },
        ];
        let prompt = build_collaboration_prompt_from_panes(&panes);
        assert!(prompt.contains("already been assigned"), "mixed roles should use pre-assigned path");
    }

    #[test]
    fn collab_prompt_includes_stand_by_instruction() {
        let panes = vec![
            PaneSummary { id: 0, agent_name: "claude".into(), role_name: Some("X".into()) },
        ];
        let prompt = build_collaboration_prompt_from_panes(&panes);
        assert!(prompt.contains("WAIT for the user"), "should tell agents to wait");
        assert!(prompt.contains("Do NOT start working"), "should forbid auto-start");
    }
}
