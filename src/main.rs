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
mod commands;
mod config;
mod events;
mod log;
mod mcp;
mod metrics;
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

        // ── Input / message wait ──────────────────────────────────────────────
        let msg = tokio::select! {
            Some(m) = event_rx.recv() => Some(m),
            _ = tokio::time::sleep(tick_duration) => Some(Message::Tick),
        };

        let mut pending_session_resume: Option<String> = None;
        let mut pending_new_session = false;

        if let Some(m) = msg {
            // Intercept Enter on the dashboard to spawn a RealPty session.
            if let Message::Key(ref key) = m {
                if key.code == crossterm::event::KeyCode::Enter {
                    if let AppScreen::Dashboard(ref dash) = state.screen {
                        if !dash.available_agents.is_empty() {
                            let agent_info = dash.available_agents[dash.selected_agent].clone();
                            if agent_info.available {
                                tracing::info!("Dashboard Enter: launching {}", agent_info.name);
                                match spawn_claude_pane(state, None) {
                                    Ok(id) => {
                                        tracing::info!("Dashboard spawned pane for session: {}", id);
                                    }
                                    Err(e) => {
                                        tracing::error!("Dashboard spawn failed: {e}");
                                        state.set_error(format!("Failed to launch {}: {}", agent_info.name, e), 100);
                                    }
                                }

                                state.tick_count = state.tick_count.wrapping_add(1);
                                continue;
                            }
                        }
                    }
                }

                // Dashboard Tab / Arrow key navigation.
                if let AppScreen::Dashboard(ref mut dash) = state.screen {
                    match key.code {
                        KeyCode::Tab => {
                            dash.focus = match dash.focus {
                                DashboardFocus::AgentList => DashboardFocus::SessionList,
                                DashboardFocus::SessionList => DashboardFocus::AgentList,
                            };
                            continue;
                        }
                        KeyCode::Up | KeyCode::Char('k') => {
                            match dash.focus {
                                DashboardFocus::AgentList if dash.selected_agent > 0 => {
                                    dash.selected_agent -= 1;
                                }
                                DashboardFocus::SessionList if dash.selected_session > 0 => {
                                    dash.selected_session -= 1;
                                }
                                _ => {}
                            }
                            continue;
                        }
                        KeyCode::Down | KeyCode::Char('j') => {
                            match dash.focus {
                                DashboardFocus::AgentList => {
                                    let max = dash.available_agents.len().saturating_sub(1);
                                    if dash.selected_agent < max { dash.selected_agent += 1; }
                                }
                                DashboardFocus::SessionList => {
                                    let max = dash.recent_sessions.len().saturating_sub(1);
                                    if dash.selected_session < max { dash.selected_session += 1; }
                                }
                            }
                            continue;
                        }
                        KeyCode::Char('q') | KeyCode::Esc => {
                            state.should_quit = true;
                            break;
                        }
                        _ => {}
                    }
                }

                // ── Session key handling ──────────────────────────────────────
                if matches!(state.screen, AppScreen::Session(_)) {
                    // ── Global quit — always intercepted first ────────────────
                    if key.modifiers.contains(KeyModifiers::CONTROL) {
                        if matches!(key.code, KeyCode::Char('q') | KeyCode::Char('\\')) {
                            state.should_quit = true;
                            break;
                        }
                    }

                    // ── Ctrl+J — jump directly to Terminal focus ──────────────
                    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('j') {
                        if let AppScreen::Session(ref mut session) = state.screen {
                            session.cockpit_focus = CockpitFocus::Terminal;
                        }
                        continue;
                    }

                    // Get current focus (without mutably borrowing state yet).
                    let current_focus = state
                        .session()
                        .map(|s| s.cockpit_focus)
                        .unwrap_or(CockpitFocus::Input);

                    // ── Tab / Shift+Tab — cycle focus ring ────────────────────
                    // When multiple panes exist and focus is on Terminal, Tab
                    // switches to the next pane's terminal before advancing to
                    // Sidebar. Shift+Tab does the reverse.
                    if key.code == KeyCode::Tab {
                        let forward = !key.modifiers.contains(KeyModifiers::SHIFT);
                        let n_panes = state.panes.len();

                        if n_panes > 1 && current_focus == CockpitFocus::Terminal {
                            let active = state.panes.active_index();
                            if forward {
                                // If not on the last pane, move to next pane.
                                if active + 1 < n_panes {
                                    state.panes.focus_next();
                                    continue;
                                }
                                // Last pane → advance focus ring normally (Terminal → Sidebar).
                            } else {
                                // If not on the first pane, move to prev pane.
                                if active > 0 {
                                    state.panes.focus_prev();
                                    continue;
                                }
                                // First pane → retreat focus ring normally (Terminal → Input).
                            }
                        }

                        // When tabbing *into* Terminal with multiple panes,
                        // land on the first pane (forward) or last pane (backward).
                        if n_panes > 1 {
                            if forward && current_focus == CockpitFocus::Input {
                                state.panes.focus(0);
                            } else if !forward && current_focus == CockpitFocus::Sidebar {
                                state.panes.focus(n_panes - 1);
                            }
                        }

                        if let AppScreen::Session(ref mut session) = state.screen {
                            session.cockpit_focus = if forward {
                                session.cockpit_focus.next()
                            } else {
                                session.cockpit_focus.prev()
                            };
                        }
                        continue;
                    }

                    // ── ? — toggle help overlay ───────────────────────────────
                    if key.code == KeyCode::Char('?') && !key.modifiers.contains(KeyModifiers::CONTROL) {
                        if let AppScreen::Session(ref mut session) = state.screen {
                            if session.overlay.is_some() {
                                session.overlay = None;
                            } else {
                                session.overlay = Some(crate::app::state::Overlay::Help);
                            }
                        }
                        continue;
                    }

                    // ── Overlay active — dispatch key to overlay ──────────────
                    {
                        let overlay_kind = state.session().and_then(|s| s.overlay.clone());
                        if overlay_kind.is_some() {
                            match &overlay_kind {
                                Some(crate::app::state::Overlay::AgentPicker) => {
                                    match key.code {
                                        KeyCode::Esc => {
                                            if let AppScreen::Session(ref mut session) = state.screen {
                                                session.overlay = None;
                                            }
                                        }
                                        KeyCode::Up | KeyCode::Char('k') => {
                                            if let AppScreen::Session(ref mut session) = state.screen {
                                                if session.agent_picker.selected > 0 {
                                                    session.agent_picker.selected -= 1;
                                                }
                                            }
                                        }
                                        KeyCode::Down | KeyCode::Char('j') => {
                                            // 3 agents: Claude, Codex, OpenCode
                                            const MAX_AGENTS: usize = 2;
                                            if let AppScreen::Session(ref mut session) = state.screen {
                                                if session.agent_picker.selected < MAX_AGENTS {
                                                    session.agent_picker.selected += 1;
                                                }
                                            }
                                        }
                                        KeyCode::Enter => {
                                            // Launch the selected agent.
                                            let selected = state.session()
                                                .map(|s| s.agent_picker.selected)
                                                .unwrap_or(0);
                                            let agent_names = ["claude", "codex", "opencode"];
                                            let adapter = agent_names.get(selected)
                                                .copied()
                                                .unwrap_or("claude");
                                            if let AppScreen::Session(ref mut session) = state.screen {
                                                session.overlay = None;
                                            }
                                            match spawn_agent_pane(state, adapter, None) {
                                                Ok(id) => tracing::info!("Agent picker launched {} pane: {}", adapter, id),
                                                Err(e) => state.set_error(format!("Failed to launch {adapter}: {e}"), 100),
                                            }
                                        }
                                        _ => {}
                                    }
                                    continue;
                                }
                                _ => {
                                    // Esc or ? dismisses all other overlays; all keys are consumed.
                                    if key.code == KeyCode::Esc || key.code == KeyCode::Char('?') {
                                        if let AppScreen::Session(ref mut session) = state.screen {
                                            session.overlay = None;
                                        }
                                    }
                                    continue;
                                }
                            }
                        }
                    }

                    // ── Esc — context-sensitive ───────────────────────────────
                    if key.code == KeyCode::Esc {
                        match current_focus {
                            // Esc from Input = close active pane; return to dashboard if no panes left.
                            CockpitFocus::Input => {
                                // Close the active pane (drops PTY).
                                state.panes.close_active();

                                // Clean up .mcp.json when dropping below 2 panes.
                                if state.panes.len() < 2 {
                                    if let Ok(cwd) = std::env::current_dir() {
                                        let _ = crate::mcp::config_writer::remove_mcp_config(&cwd);
                                    }
                                }

                                // Also clear legacy fields.
                                turn_handle = None;
                                state.real_pty = None;
                                state.claude_log = None;

                                if state.panes.is_empty() {
                                    state.screen = AppScreen::Dashboard(DashboardState {
                                        available_agents: detect_agents(),
                                        ..DashboardState::default()
                                    });
                                }
                                continue;
                            }
                            // Esc from anything else = return focus to Input.
                            _ => {
                                if let AppScreen::Session(ref mut session) = state.screen {
                                    session.cockpit_focus = CockpitFocus::Input;
                                }
                                continue;
                            }
                        }
                    }

                    // ── Terminal focus — viewport scroll first, PTY keys second ──
                    if current_focus == CockpitFocus::Terminal {
                        {
                            let has_pane = !state.panes.is_empty();
                            let handled = if has_pane {
                                if let Some(pane) = state.panes.active_pane_mut() {
                                    match key.code {
                                        KeyCode::PageUp   => { pane.session.scroll_terminal_up(10);    true }
                                        KeyCode::PageDown => { pane.session.scroll_terminal_down(10);  true }
                                        KeyCode::Home     => { pane.session.scroll_terminal_up(10_000); true }
                                        KeyCode::End      => { pane.session.reset_terminal_scroll();   true }
                                        _ => false,
                                    }
                                } else { false }
                            } else if let Some(session) = state.session_mut() {
                                match key.code {
                                    KeyCode::PageUp   => { session.scroll_terminal_up(10);    true }
                                    KeyCode::PageDown => { session.scroll_terminal_down(10);  true }
                                    KeyCode::Home     => { session.scroll_terminal_up(10_000); true }
                                    KeyCode::End      => { session.reset_terminal_scroll();   true }
                                    _ => false,
                                }
                            } else { false };
                            if handled { continue; }
                        }

                        let raw_bytes = key_event_to_bytes(*key);
                        if !raw_bytes.is_empty() {
                            // Write to active pane's PTY.
                            if let Some(pane) = state.panes.active_pane_mut() {
                                if let Some(ref mut pty) = pane.pty {
                                    if let Err(e) = pty.write_input(&raw_bytes) {
                                        tracing::warn!("PTY write_input (terminal focus): {e}");
                                    }
                                }
                            } else if let Some(ref mut pty) = state.real_pty {
                                // Legacy fallback.
                                if let Err(e) = pty.write_input(&raw_bytes) {
                                    tracing::warn!("PTY write_input (terminal focus, legacy): {e}");
                                }
                            }
                        }
                        continue;
                    }

                    // ── Input focus — Potato-owned text editing ───────────────
                    if current_focus == CockpitFocus::Input {
                        // ── Autocomplete navigation (Up/Down/Tab) ────────────
                        // Only active when input starts with `/`.
                        let in_command_mode = state.session()
                            .map(|s| s.input_buffer.starts_with('/'))
                            .unwrap_or(false);

                        if in_command_mode {
                            match key.code {
                                KeyCode::Up => {
                                    if let AppScreen::Session(ref mut session) = state.screen {
                                        let prefix = &session.input_buffer[1..];
                                        let count = commands::registry::completions(prefix).len();
                                        if count > 0 {
                                            if session.command_selected == 0 {
                                                session.command_selected = count - 1;
                                            } else {
                                                session.command_selected -= 1;
                                            }
                                        }
                                    }
                                    continue;
                                }
                                KeyCode::Down => {
                                    if let AppScreen::Session(ref mut session) = state.screen {
                                        let prefix = &session.input_buffer[1..];
                                        let count = commands::registry::completions(prefix).len();
                                        if count > 0 {
                                            session.command_selected = (session.command_selected + 1) % count;
                                        }
                                    }
                                    continue;
                                }
                                KeyCode::Tab => {
                                    if let AppScreen::Session(ref mut session) = state.screen {
                                        let prefix = session.input_buffer[1..].to_string();
                                        let completions = commands::registry::completions(&prefix);
                                        let idx = session.command_selected.min(completions.len().saturating_sub(1));
                                        if let Some(cmd) = completions.get(idx) {
                                            session.input_buffer = format!("/{}", cmd.name);
                                            session.input_cursor = session.input_buffer.len();
                                            session.command_selected = 0;
                                        }
                                    }
                                    continue;
                                }
                                _ => {}
                            }
                        }

                        if let AppScreen::Session(ref mut session) = state.screen {
                            match key.code {
                                // Enter — parse slash command or send to PTY.
                                KeyCode::Enter => {
                                    let text = std::mem::take(&mut session.input_buffer);
                                    session.input_cursor = 0;
                                    session.command_selected = 0;
                                    session.reset_terminal_scroll();

                                    if !text.is_empty() {
                                        if text.starts_with('/') {
                                            // ── Slash command dispatch ────────
                                            use commands::registry::{CommandResult, OverlayKind};
                                            match commands::registry::parse_command(&text) {
                                                CommandResult::ShowOverlay(OverlayKind::Help) => {
                                                    session.overlay = Some(crate::app::state::Overlay::Help);
                                                }
                                                CommandResult::ShowOverlay(OverlayKind::Sessions) => {
                                                    session.overlay = Some(crate::app::state::Overlay::Sessions);
                                                }
                                                CommandResult::ShowOverlay(OverlayKind::AgentPicker) => {
                                                    session.overlay = Some(crate::app::state::Overlay::AgentPicker);
                                                }
                                                CommandResult::NewSession { .. } => {
                                                    pending_new_session = true;
                                                }
                                                CommandResult::SetRole { name, description } => {
                                                    // Store role on the active pane.
                                                    let active_idx = state.panes.active_index();
                                                    let active_pane_id = state.panes.active_pane().map(|p| p.id);
                                                    if let Some(pane) = state.panes.active_pane_mut() {
                                                        pane.role_name = Some(name.clone());
                                                        pane.role_description = description.clone();
                                                        tracing::info!(
                                                            "Pane {} role set to '{}': {:?}",
                                                            pane.id, name, description
                                                        );
                                                    }
                                                    // Inject notification into ALL other panes.
                                                    if let Some(pid) = active_pane_id {
                                                        let n_panes = state.panes.len();
                                                        let role_ref: Option<&str> = Some(&name);
                                                        let notification = crate::mcp::injection::format_notification(
                                                            pid,
                                                            role_ref,
                                                            &description.clone().unwrap_or_default(),
                                                        );
                                                        for target in 0..n_panes {
                                                            if state.panes.get(target).map(|p| p.id) != Some(pid) {
                                                                if let Err(e) = crate::mcp::injection::inject_into_pane(
                                                                    &mut state.panes,
                                                                    target,
                                                                    &notification,
                                                                ) {
                                                                    tracing::warn!("role inject to pane {target}: {e}");
                                                                }
                                                            }
                                                        }
                                                    }
                                                }
                                                CommandResult::Handled => {
                                                    tracing::info!("Slash command handled: {}", text);
                                                }
                                                CommandResult::Unknown(cmd) => {
                                                    state.set_error(
                                                        format!("Unknown command: /{cmd}  (type /help for commands)"),
                                                        80,
                                                    );
                                                }
                                                CommandResult::PassThrough(_) => {
                                                    // Should not happen since we checked starts_with('/').
                                                }
                                            }
                                        } else {
                                            // ── Normal PTY send ───────────────
                                            let written = if let Some(pane) = state.panes.active_pane_mut() {
                                                if let Some(ref mut pty) = pane.pty {
                                                    if let Err(e) = pty.write_input(text.as_bytes()) {
                                                        tracing::warn!("PTY write_input (text): {e}");
                                                        false
                                                    } else if let Err(e) = pty.write_input(b"\r") {
                                                        tracing::warn!("PTY write_input (enter): {e}");
                                                        false
                                                    } else {
                                                        true
                                                    }
                                                } else { false }
                                            } else { false };
                                            // Legacy fallback.
                                            if !written {
                                                if let Some(ref mut pty) = state.real_pty {
                                                    if let Err(e) = pty.write_input(text.as_bytes()) {
                                                        tracing::warn!("PTY write_input (text, legacy): {e}");
                                                    } else if let Err(e) = pty.write_input(b"\r") {
                                                        tracing::warn!("PTY write_input (enter, legacy): {e}");
                                                    }
                                                }
                                            }
                                        }
                                    }
                                    continue;
                                }
                                KeyCode::Backspace => {
                                    session.input_buffer.pop();
                                    if session.input_cursor > session.input_buffer.len() {
                                        session.input_cursor = session.input_buffer.len();
                                    }
                                    // Reset autocomplete selection on buffer change.
                                    session.command_selected = 0;
                                    continue;
                                }
                                KeyCode::Left => {
                                    if session.input_cursor > 0 {
                                        session.input_cursor -= 1;
                                    }
                                    continue;
                                }
                                KeyCode::Right => {
                                    if session.input_cursor < session.input_buffer.len() {
                                        session.input_cursor += 1;
                                    }
                                    continue;
                                }
                                KeyCode::Home => {
                                    session.input_cursor = 0;
                                    continue;
                                }
                                KeyCode::End => {
                                    session.input_cursor = session.input_buffer.len();
                                    continue;
                                }
                                KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                                    session.input_buffer.push(c);
                                    session.input_cursor = session.input_buffer.len();
                                    // Reset autocomplete selection when buffer changes.
                                    session.command_selected = 0;
                                    continue;
                                }
                                _ => {}
                            }
                        }
                    }

                    // ── Agents focus — agent picker ───────────────────────────
                    if current_focus == CockpitFocus::Agents {
                        if let AppScreen::Session(ref mut session) = state.screen {
                            let agent_count = crate::ui::overlays::agent_picker::build_agent_rows().len();
                            let max_idx = agent_count.saturating_sub(1);
                            match key.code {
                                KeyCode::Up | KeyCode::Char('k') => {
                                    if session.selected_agent > 0 {
                                        session.selected_agent -= 1;
                                    }
                                    continue;
                                }
                                KeyCode::Down | KeyCode::Char('j') => {
                                    if session.selected_agent < max_idx {
                                        session.selected_agent += 1;
                                    }
                                    continue;
                                }
                                KeyCode::Home => {
                                    session.selected_agent = 0;
                                    continue;
                                }
                                KeyCode::End => {
                                    session.selected_agent = max_idx;
                                    continue;
                                }
                                KeyCode::Enter => {
                                    // Spawn agent session for the selected agent.
                                    pending_new_session = true;
                                    // Fall through so the deferred handler runs.
                                }
                                _ => {}
                            }
                        }
                    }

                    // ── Sessions focus — navigate the session list ────────────
                    if current_focus == CockpitFocus::Sessions {
                        if let AppScreen::Session(ref mut session) = state.screen {
                            let max_idx = state.rail_sessions.len().saturating_sub(1);
                            match key.code {
                                KeyCode::Up | KeyCode::Char('k') => {
                                    if session.selected_session > 0 {
                                        session.selected_session -= 1;
                                    }
                                    continue;
                                }
                                KeyCode::Down | KeyCode::Char('j') => {
                                    if session.selected_session < max_idx {
                                        session.selected_session += 1;
                                    }
                                    continue;
                                }
                                KeyCode::Home => {
                                    session.selected_session = 0;
                                    continue;
                                }
                                KeyCode::End => {
                                    session.selected_session = max_idx;
                                    continue;
                                }
                                KeyCode::PageUp => {
                                    session.selected_session = session.selected_session.saturating_sub(10);
                                    continue;
                                }
                                KeyCode::PageDown => {
                                    session.selected_session = (session.selected_session + 10).min(max_idx);
                                    continue;
                                }
                                KeyCode::Enter => {
                                    // Load the selected historical session.
                                    if let Some(info) = state.rail_sessions.get(session.selected_session) {
                                        pending_session_resume = Some(info.id.clone());
                                    }
                                    // Fall through (no continue) so the deferred
                                    // resume handler runs at the end of this iteration.
                                }
                                _ => {}
                            }
                        }
                    }

                    // ── Sidebar focus — navigate sidebar items ────────────────
                    if current_focus == CockpitFocus::Sidebar {
                        if let AppScreen::Session(ref mut session) = state.screen {
                            match key.code {
                                KeyCode::Enter => {
                                    session.cockpit_focus = CockpitFocus::Input;
                                    continue;
                                }
                                _ => {}
                            }
                        }
                    }
                }
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
                state.panes.close(i);
            }
            // Clean up .mcp.json when dropping below 2 panes.
            if had_panes && state.panes.len() < 2 {
                if let Ok(cwd) = std::env::current_dir() {
                    if let Err(e) = crate::mcp::config_writer::remove_mcp_config(&cwd) {
                        tracing::warn!("Failed to clean up .mcp.json: {e}");
                    } else {
                        tracing::info!("Cleaned up .mcp.json (panes < 2)");
                    }
                }
            }
            // Only bounce to dashboard if we just closed the last pane.
            if had_panes && state.panes.is_empty() && matches!(state.screen, AppScreen::Session(_)) {
                tracing::info!("All panes closed, returning to dashboard");
                state.real_pty = None;
                state.claude_log = None;
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

        if state.should_quit {
            break;
        }
    }

    Ok(())
}

/// Apply a PTY event to the application state.
///
/// Delegates to the pure [`app::session_reducer::apply_event`] function so all
/// state-transition logic is unit-testable without a terminal or PTY.
fn sync_claude_log(state: &mut AppState) {
    let Some(tracker) = state.claude_log.as_mut() else { return; };
    let Ok(changed) = tracker.poll() else { return; };
    if !changed {
        return;
    }

    let snapshot = tracker.snapshot();

    // ── Update live sidebar metrics ───────────────────────────────────────────
    if let Some(session) = state.session_mut() {
        session.metrics.input_tokens = snapshot.usage.input_tokens;
        session.metrics.output_tokens = snapshot.usage.output_tokens;
        session.tokens_used = snapshot.usage.total_tokens();
    }

    // ── Persist to SQLite ─────────────────────────────────────────────────────
    let store = match state.store.clone() {
        Some(s) => s,
        None => return,
    };

    let session_id = state
        .session()
        .and_then(|s| s.claude_session_id.clone())
        .unwrap_or_default();
    if session_id.is_empty() {
        return;
    }

    let project_dir = if let Some(home) = dirs::home_dir() {
        let cwd = std::env::current_dir().ok().unwrap_or_else(|| home.clone());
        crate::claude_log::project_dir_name(&cwd)
    } else {
        String::new()
    };

    let now = unix_now();

    // Use the title from the JSONL tracker (first user prompt).
    // Fall back to existing rail title if the tracker hasn't seen one yet.
    let title = if !snapshot.title.is_empty() {
        snapshot.title.clone()
    } else {
        state
            .rail_sessions
            .iter()
            .find(|s| s.id == session_id)
            .map(|s| s.title.clone())
            .unwrap_or_default()
    };

    // Track whether title changed so we can force a rail refresh.
    let old_title = state
        .rail_sessions
        .iter()
        .find(|s| s.id == session_id)
        .map(|s| s.title.clone())
        .unwrap_or_default();
    let title_changed = title != old_title;

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
        now, // created_at — ON CONFLICT keeps the original via MAX
        now,
    ) {
        tracing::warn!("sync_claude_log: upsert_session failed: {e}");
    }

    // Refresh the rail on title change, or every 30 s, or on first run.
    let elapsed = now - state.last_rail_refresh;
    if title_changed || elapsed >= 30 || state.last_rail_refresh == 0 {
        refresh_rail(state, &store);
    }
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
        (rid.to_string(), vec!["--resume".into(), rid.into()])
    } else {
        let id = uuid::Uuid::new_v4().to_string();
        let args = vec!["--session-id".into(), id.clone()];
        (id, args)
    };

    let session_args_refs: Vec<&str> = session_args_owned.iter().map(|s| s.as_str()).collect();

    // ── MCP env vars (2nd pane and beyond) ───────────────────────────────────
    // When a socket is available and we're spawning pane 1+, pass env vars so
    // Claude can connect to the MCP bridge.
    let pane_index_after_open = state.panes.len(); // 0-based index the new pane will occupy
    let mut pane_env: Vec<(String, String)> = Vec::new();
    if let Some(ref sock) = state.mcp_socket_path.clone() {
        // Always provide the socket path and the future pane id to every pane.
        // The pane id matches `PaneManager::next_id` — we can read it from the manager.
        // PaneManager doesn't expose next_id directly, so we derive it:
        // after open(), `len()` panes exist and active pane has id == (old_len).
        // We'll set the env now and the id is the value of `state.panes.len()` since
        // ids are monotonically allocated from 0 and never reused in a single session.
        let pane_id: u64 = state.panes.len() as u64; // speculative — matches next_id
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

    let _dirty_rx = real_pty.dirty_tx.subscribe();
    pane.pty = Some(real_pty);
    pane.session.status = crate::app::state::AgentStatus::Idle;
    pane.session.claude_session_id = Some(session_id.clone());

    // Set up JSONL log tracker.
    if let Some(home) = dirs::home_dir() {
        let cwd = launch_cwd.as_deref().unwrap_or(&home);
        let path = crate::claude_log::session_log_path(&home, cwd, &session_id);
        tracing::info!("Claude session log: {}", path.display());
        pane.log = Some(crate::claude_log::ClaudeSessionLogTracker::new(path));
    }

    // Mirror to legacy fields for compatibility during migration.
    // TODO: remove once all reads go through panes.
    let active_idx = state.panes.active_index();
    if let Some(p) = state.panes.get(active_idx) {
        // We can't easily move the PTY, so clone the log state and leave real_pty as the
        // primary on the pane. For legacy code paths that read state.real_pty, we skip
        // the mirror — they'll be migrated next.
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

    // ── Write .mcp.json when the 2nd pane is opened ───────────────────────────
    // When we now have ≥ 2 panes and a socket path, write/update .mcp.json so
    // Claude can discover the MCP server entries.
    if state.panes.len() >= 2 {
        if let Some(ref sock) = state.mcp_socket_path.clone() {
            if let Some(ref cwd) = launch_cwd {
                let pane_ids: Vec<u64> =
                    (0..state.panes.len()).filter_map(|i| state.panes.get(i).map(|p| p.id)).collect();
                let sock_str = sock.to_string_lossy();
                if let Err(e) = crate::mcp::config_writer::write_mcp_config(cwd, &pane_ids, &sock_str) {
                    tracing::warn!("Failed to write .mcp.json: {e}");
                } else {
                    tracing::info!("Wrote .mcp.json for panes: {:?}", pane_ids);
                }
            }
        }
    }

    tracing::info!("Opened Claude pane for session: {}", session_id);
    Ok(session_id)
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

    // Also sync legacy single tracker during migration.
    sync_claude_log(state);

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

    // Pre-load session list for the left rail.
    let initial_sessions = store.list_sessions().unwrap_or_default();

    // Start MCP bridge (UDS listener for inter-session communication).
    let inter_state = Arc::new(std::sync::Mutex::new(mcp::state::InterSessionState::new()));
    let (_mcp_bridge, mcp_socket_path) = mcp::bridge::McpBridge::start(Arc::clone(&inter_state))?;

    let mut state = AppState {
        model,
        screen: AppScreen::Dashboard(DashboardState {
            available_agents: agents,
            ..DashboardState::default()
        }),
        store: Some(store),
        rail_sessions: initial_sessions,
        last_rail_refresh: unix_now(),
        mcp_socket_path: Some(mcp_socket_path),
        ..AppState::default()
    };

    // Enter TUI.
    let _guard = TerminalGuard::enter()?;
    let mut terminal = ratatui::init();

    let result = run_async(&mut terminal, &mut state).await;

    ratatui::restore();
    result
}
