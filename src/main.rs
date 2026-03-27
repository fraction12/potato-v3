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
use session::SessionStore;
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
        sync_claude_log(state);

        // ── Input / message wait ──────────────────────────────────────────────
        let msg = tokio::select! {
            Some(m) = event_rx.recv() => Some(m),
            _ = tokio::time::sleep(tick_duration) => Some(Message::Tick),
        };

        if let Some(m) = msg {
            // Intercept Enter on the dashboard to spawn a RealPty session.
            if let Message::Key(ref key) = m {
                if key.code == crossterm::event::KeyCode::Enter {
                    if let AppScreen::Dashboard(ref dash) = state.screen {
                        if !dash.available_agents.is_empty() {
                            let agent_info = dash.available_agents[dash.selected_agent].clone();
                            if agent_info.available {
                                let binary = agent_info.binary_path.clone().unwrap();
                                let agent_name = agent_info.name.clone();

                                // ── RealPty cockpit launch ─────────────────────────────
                                // Estimate PTY size from terminal dimensions.
                                // The output panel is ~75% wide, and minus the status/
                                // sessions/input bars (~7 lines) for height.
                                let (term_cols, term_rows) =
                                    crossterm::terminal::size().unwrap_or((120, 40));
                                let pty_cols = (term_cols as u32 * 3 / 4).saturating_sub(2) as u16;
                                let pty_rows = term_rows.saturating_sub(10);

                                tracing::info!(
                                    "Launching {} via RealPty at {}×{}",
                                    agent_name, pty_cols, pty_rows,
                                );

                                let session_id = Uuid::new_v4().to_string();
                                let session_args = ["--session-id", session_id.as_str()];
                                let launch_cwd = std::env::current_dir().ok();

                                match crate::pty::RealPty::spawn_in(
                                    &binary.to_string_lossy(),
                                    &session_args,
                                    pty_cols.max(20),
                                    pty_rows.max(5),
                                    launch_cwd.as_deref(),
                                ) {
                                    Ok(real_pty) => {
                                        // Subscribe to dirty notifications for re-render.
                                        let mut _dirty_rx = real_pty.dirty_tx.subscribe();
                                        state.real_pty = Some(real_pty);

                                        // Transition to session screen.
                                        state.enter_session(&session_id, &agent_name);

                                        if let Some(home) = dirs::home_dir() {
                                            let cwd = launch_cwd.as_deref().unwrap_or(&home);
                                            let path = crate::claude_log::session_log_path(&home, cwd, &session_id);
                                            tracing::info!("Claude log path: {}", path.display());
                                            state.claude_log = Some(crate::claude_log::ClaudeSessionLogTracker::new(path));
                                        }

                                        // Mark session as idle (PTY is live).
                                        if let Some(ref mut session) = state.session_mut() {
                                            session.status = crate::app::state::AgentStatus::Idle;
                                            session.claude_session_id = Some(session_id.clone());
                                        }
                                    }
                                    Err(e) => {
                                        tracing::error!("RealPty spawn failed: {e}");
                                        // Remain on dashboard; surface error via state.
                                        state.set_error(format!("Failed to launch {}: {}", agent_name, e), 100);
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

                    // ── Esc — context-sensitive ───────────────────────────────
                    if key.code == KeyCode::Esc {
                        match current_focus {
                            // Esc from Input = return to dashboard, drop PTY.
                            CockpitFocus::Input => {
                                turn_handle = None;
                                state.real_pty = None;
                                state.claude_log = None;
                                state.screen = AppScreen::Dashboard(DashboardState {
                                    available_agents: detect_agents(),
                                    ..DashboardState::default()
                                });
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
                        if let Some(session) = state.session_mut() {
                            match key.code {
                                KeyCode::PageUp => {
                                    session.scroll_terminal_up(10);
                                    continue;
                                }
                                KeyCode::PageDown => {
                                    session.scroll_terminal_down(10);
                                    continue;
                                }
                                KeyCode::Home => {
                                    session.scroll_terminal_up(10_000);
                                    continue;
                                }
                                KeyCode::End => {
                                    session.reset_terminal_scroll();
                                    continue;
                                }
                                _ => {}
                            }
                        }

                        let raw_bytes = key_event_to_bytes(*key);
                        if !raw_bytes.is_empty() {
                            if let Some(ref mut pty) = state.real_pty {
                                if let Err(e) = pty.write_input(&raw_bytes) {
                                    tracing::warn!("PTY write_input (terminal focus): {e}");
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
                                        if let Some(ref mut pty) = state.real_pty {
                                            if let Err(e) = pty.write_input(text.as_bytes()) {
                                                tracing::warn!("PTY write_input (text): {e}");
                                            } else if let Err(e) = pty.write_input(b"\r") {
                                                tracing::warn!("PTY write_input (enter): {e}");
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

                    // ── Sessions focus — navigate the session list ────────────
                    if current_focus == CockpitFocus::Sessions {
                        if let AppScreen::Session(ref mut session) = state.screen {
                            match key.code {
                                KeyCode::Up | KeyCode::Char('k') => {
                                    if session.selected_session > 0 {
                                        session.selected_session -= 1;
                                    }
                                    continue;
                                }
                                KeyCode::Down | KeyCode::Char('j') => {
                                    session.selected_session = session.selected_session.saturating_add(1);
                                    continue;
                                }
                                KeyCode::Enter => {
                                    // Switch to focused session (placeholder for multi-session).
                                    session.cockpit_focus = CockpitFocus::Input;
                                    continue;
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
                        if let Some(session) = state.session_mut() {
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

            // Standard update/action dispatch.
            let action = update(state, m);
            use app::action::Action;
            if matches!(action, Action::Quit) {
                break;
            }
        }

        // Tick counter (drives animations).
        state.tick_count = state.tick_count.wrapping_add(1);
        let tc = state.tick_count;
        if let Some(session) = state.session_mut() {
            session.tick_count = tc;
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
    if let Some(session) = state.session_mut() {
        session.metrics.input_tokens = snapshot.usage.input_tokens;
        session.metrics.output_tokens = snapshot.usage.output_tokens;
        session.tokens_used = snapshot.usage.total_tokens();
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
    let _store = SessionStore::open(&db_path.to_string_lossy())?;

    // Build initial state with detected agents.
    let agents = detect_agents();
    // The --model flag is stored in AppState directly (Claude picks its own model;
    // this is only used for display purposes in the status bar).
    let model = cli.model.unwrap_or_else(|| cfg.default_agent.clone());
    let mut state = AppState {
        model,
        screen: AppScreen::Dashboard(DashboardState {
            available_agents: agents,
            ..DashboardState::default()
        }),
        ..AppState::default()
    };

    // Enter TUI.
    let _guard = TerminalGuard::enter()?;
    let mut terminal = ratatui::init();

    let result = run_async(&mut terminal, &mut state).await;

    ratatui::restore();
    result
}
