//! Potato — terminal cockpit for external coding agents.
//!
//! Boots to a dashboard where you pick an agent, then suspends its TUI,
//! hands the full terminal to the agent, and reclaims it when the agent exits.

// Scaffold: suppress warnings for types/items not yet fully wired.
#![allow(dead_code, unused_imports, unused_variables)]

mod adapters;
mod app;
mod claude_log;
mod config;
mod events;
mod log;
mod metrics;
mod pty;
mod session;
mod terminal;
mod ui;

use std::io::{self, Write as _};
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use clap::Parser;
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
use adapters::{AgentAdapter, claude::ClaudeAdapter, generic::GenericAdapter};
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
    agents.push(AgentInfo {
        name: "Claude Code".to_string(),
        adapter: "claude".to_string(),
        binary_path: claude.detect(),
        available: claude.detect().is_some(),
    });

    // Codex (generic adapter for now)
    let codex = GenericAdapter::new("codex");
    agents.push(AgentInfo {
        name: "Codex".to_string(),
        adapter: "codex".to_string(),
        binary_path: codex.detect(),
        available: codex.detect().is_some(),
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
                    if key.code == KeyCode::Tab {
                        if let AppScreen::Session(ref mut session) = state.screen {
                            session.cockpit_focus = if key.modifiers.contains(KeyModifiers::SHIFT) {
                                session.cockpit_focus.prev()
                            } else {
                                session.cockpit_focus.next()
                            };
                        }
                        continue;
                    }

                    // ── Ctrl+Left / Ctrl+Right — switch active pane ──────────
                    if key.modifiers.contains(KeyModifiers::CONTROL) && state.panes.len() > 1 {
                        match key.code {
                            KeyCode::Left => {
                                state.panes.focus_prev();
                                continue;
                            }
                            KeyCode::Right => {
                                state.panes.focus_next();
                                continue;
                            }
                            _ => {}
                        }
                    }

                    // ── Esc — context-sensitive ───────────────────────────────
                    if key.code == KeyCode::Esc {
                        match current_focus {
                            // Esc from Input = close active pane; return to dashboard if no panes left.
                            CockpitFocus::Input => {
                                // Close the active pane (drops PTY).
                                state.panes.close_active();

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
                        if let AppScreen::Session(ref mut session) = state.screen {
                            match key.code {
                                // Enter — send input_buffer, then a real terminal Enter (CR) to PTY.
                                KeyCode::Enter => {
                                    let text = std::mem::take(&mut session.input_buffer);
                                    session.input_cursor = 0;
                                    session.reset_terminal_scroll();
                                    if !text.is_empty() {
                                        // Write to active pane's PTY.
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
                                    continue;
                                }
                                KeyCode::Backspace => {
                                    session.input_buffer.pop();
                                    if session.input_cursor > session.input_buffer.len() {
                                        session.input_cursor = session.input_buffer.len();
                                    }
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
                                    continue;
                                }
                                _ => {}
                            }
                        }
                    }

                    // ── Agents focus — agent picker ───────────────────────────
                    if current_focus == CockpitFocus::Agents {
                        if let AppScreen::Session(ref mut _session) = state.screen {
                            match key.code {
                                KeyCode::Enter => {
                                    // Spawn a new Claude session.
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

        // ── Spawn a new Claude session (deferred from Agents Enter) ─────────
        if pending_new_session {
            match spawn_claude_pane(state, None) {
                Ok(id) => tracing::info!("New pane spawned: {}", id),
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

    let real_pty = crate::pty::RealPty::spawn_in(
        binary.to_str().unwrap_or("claude"),
        &session_args_refs,
        pty_cols.max(20),
        pty_rows.max(5),
        launch_cwd.as_deref(),
    )
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

    tracing::info!("Opened Claude pane for session: {}", session_id);
    Ok(session_id)
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

    let mut state = AppState {
        model,
        screen: AppScreen::Dashboard(DashboardState {
            available_agents: agents,
            ..DashboardState::default()
        }),
        store: Some(store),
        rail_sessions: initial_sessions,
        last_rail_refresh: unix_now(),
        ..AppState::default()
    };

    // Enter TUI.
    let _guard = TerminalGuard::enter()?;
    let mut terminal = ratatui::init();

    let result = run_async(&mut terminal, &mut state).await;

    ratatui::restore();
    result
}
