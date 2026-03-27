//! Potato — terminal cockpit for external coding agents.
//!
//! Boots to a dashboard where you pick an agent, then hosts it in a rich
//! PTY cockpit that wraps the live session.

// Scaffold: suppress warnings for types/items not yet fully wired.
#![allow(dead_code, unused_imports, unused_variables)]

mod adapters;
mod app;
mod config;
mod events;
mod legacy;
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
    event::{Event, KeyCode, KeyModifiers},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::DefaultTerminal;
use tracing_subscriber::EnvFilter;
use uuid::Uuid;

use app::message::Message;
use app::state::{AppScreen, AppState, DashboardFocus};
use app::update::update;
use config::load_config;
use session::SessionStore;
use terminal::events::event_stream;
use terminal::panic_hook::install_panic_hook;
use adapters::claude::ClaudeAdapter;
use adapters::generic::GenericAdapter;
use adapters::{AdapterConfig, AgentAdapter};
use app::state::{AgentInfo, DashboardState};
use ui::screens::{dashboard::render_dashboard, session::render_session};

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
        execute!(io::stdout(), EnterAlternateScreen)?;
        Ok(Self)
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
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

    // Active PTY handle (if a session is running).
    let mut pty_handle: Option<crate::pty::PtyHandle> = None;

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
                }
            }
        })?;

        // ── PTY event drain ───────────────────────────────────────────────────
        if let Some(ref mut handle) = pty_handle {
            // Drain any pending PTY events without blocking.
            loop {
                match handle.event_rx.try_recv() {
                    Ok(event) => apply_pty_event(state, event),
                    Err(_) => break,
                }
            }
            // Check if the process exited.
            if let Some(code) = *handle.exit_rx.borrow() {
                if let Some(session) = state.session_mut() {
                    session.status = app::state::AgentStatus::Exited { code: Some(code) };
                }
            }
        }

        // ── Input / message wait ──────────────────────────────────────────────
        let msg = tokio::select! {
            Some(m) = event_rx.recv() => Some(m),
            _ = tokio::time::sleep(tick_duration) => Some(Message::Tick),
        };

        if let Some(m) = msg {
            // Intercept Enter on the dashboard to launch a session.
            if let Message::Key(ref key) = m {
                if key.code == crossterm::event::KeyCode::Enter {
                    if let AppScreen::Dashboard(ref dash) = state.screen {
                        if !dash.available_agents.is_empty() {
                            let agent_info = dash.available_agents[dash.selected_agent].clone();
                            if agent_info.available {
                                // Launch the selected agent.
                                let session_id = Uuid::new_v4().to_string();
                                let adapter: Arc<dyn AgentAdapter> = match agent_info.adapter.as_str() {
                                    "claude" => Arc::new(ClaudeAdapter),
                                    other => Arc::new(GenericAdapter::new(other)),
                                };
                                let config = AdapterConfig {
                                    model: if state.model.is_empty() { None } else { Some(state.model.clone()) },
                                    ..AdapterConfig::default()
                                };

                                match crate::pty::PtyProcess::spawn(adapter, config).await {
                                    Ok(handle) => {
                                        state.enter_session(session_id, &agent_info.name);
                                        pty_handle = Some(handle);
                                    }
                                    Err(e) => {
                                        state.set_error(format!("Failed to launch {}: {}", agent_info.name, e), 80);
                                    }
                                }
                                // Don't process further for this tick.
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

                // Session key handling — send input to PTY.
                if let AppScreen::Session(ref mut session) = state.screen {
                    match key.code {
                        KeyCode::Char('q') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                            state.should_quit = true;
                            break;
                        }
                        KeyCode::Esc => {
                            // Return to dashboard.
                            state.screen = AppScreen::Dashboard(DashboardState {
                                available_agents: detect_agents(),
                                ..DashboardState::default()
                            });
                            pty_handle = None;
                            continue;
                        }
                        KeyCode::Enter => {
                            // Submit the input buffer to the agent.
                            let text = std::mem::take(&mut session.input_buffer);
                            if !text.is_empty() {
                                session.transcript.push(
                                    app::state::TranscriptEntry::user(&text),
                                );
                                if let Some(ref handle) = pty_handle {
                                    let formatted = ClaudeAdapter.format_user_input(&text);
                                    let _ = handle.stdin_tx.try_send(formatted);
                                }
                            }
                            continue;
                        }
                        KeyCode::Backspace => {
                            session.input_buffer.pop();
                            continue;
                        }
                        KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                            session.input_buffer.push(c);
                            continue;
                        }
                        _ => {}
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
fn apply_pty_event(state: &mut AppState, event: crate::events::AgentEvent) {
    use crate::events::AgentEvent;

    let Some(session) = state.session_mut() else { return };

    match event {
        AgentEvent::TextDelta { text } => {
            // Append to the last assistant transcript entry, or create one.
            match session.transcript.last_mut() {
                Some(e) if e.role == app::state::MessageRole::Assistant => {
                    e.content.push_str(&text);
                }
                _ => {
                    session.transcript.push(app::state::TranscriptEntry::assistant(&text));
                }
            }
            session.status = app::state::AgentStatus::Thinking;
        }
        AgentEvent::TextDone { full_text } => {
            // Ensure the last entry reflects the complete text.
            if let Some(e) = session.transcript.last_mut() {
                if e.role == app::state::MessageRole::Assistant {
                    e.content = full_text;
                }
            }
        }
        AgentEvent::ToolStart { id, name, input } => {
            session.status = app::state::AgentStatus::RunningTool { name: name.clone() };
            session.tool_calls.push(app::state::ToolCallRecord {
                id,
                name,
                input,
                output: None,
                started_at: chrono::Utc::now(),
                duration_ms: None,
                success: None,
            });
        }
        AgentEvent::ToolDone { id, output, duration_ms, success } => {
            if let Some(tc) = session.tool_calls.iter_mut().find(|t| t.id == id) {
                tc.output = Some(output);
                tc.duration_ms = Some(duration_ms);
                tc.success = Some(success);
            }
            session.status = app::state::AgentStatus::Thinking;
        }
        AgentEvent::ToolError { id, error } => {
            if let Some(tc) = session.tool_calls.iter_mut().find(|t| t.id == id) {
                tc.output = Some(error.clone());
                tc.success = Some(false);
            }
        }
        AgentEvent::ApprovalRequired { tool_id, tool_name, input } => {
            session.status = app::state::AgentStatus::WaitingApproval { tool_name: tool_name.clone() };
            session.approval_pending = Some(app::state::PendingApprovalSession { tool_id, tool_name, input });
        }
        AgentEvent::TurnDone { usage } => {
            session.status = app::state::AgentStatus::Idle;
            if let Some(u) = usage {
                session.metrics.input_tokens += u.input_tokens;
                session.metrics.output_tokens += u.output_tokens;
                if let Some(cost) = u.cost_usd {
                    session.metrics.total_cost_usd += cost;
                }
                session.metrics.turn_count += 1;
            }
        }
        AgentEvent::SessionBound { agent_session_id } => {
            session.session_id = agent_session_id;
        }
        AgentEvent::AgentStarted { .. } => {
            session.status = app::state::AgentStatus::Thinking;
        }
        AgentEvent::AgentExited { exit_code } => {
            session.status = app::state::AgentStatus::Exited { code: exit_code };
        }
        AgentEvent::Error { message } => {
            session.status = app::state::AgentStatus::Error { message: message.clone() };
            session.transcript.push(app::state::TranscriptEntry::system(format!("Error: {}", message)));
        }
        AgentEvent::Warning { message } => {
            tracing::warn!("{}", message);
        }
        AgentEvent::Raw { payload } => {
            // Append raw output as an assistant message for visibility.
            match session.transcript.last_mut() {
                Some(e) if e.role == app::state::MessageRole::Assistant => {
                    e.content.push_str(&payload);
                    e.content.push('\n');
                }
                _ => {
                    session.transcript.push(app::state::TranscriptEntry::assistant(&payload));
                }
            }
        }
        _ => {}
    }
}

// ── Entry point ───────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Initialise tracing.
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn")),
        )
        .with_writer(io::stderr)
        .init();

    install_panic_hook();

    // Load configuration.
    let mut cfg = load_config(cli.config.as_deref())?;
    if let Some(model) = cli.model {
        cfg.model = model;
    }

    // Initialise session store.
    let db_path = config::expand_tilde(&cfg.db_path);
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    let _store = SessionStore::open(&db_path.to_string_lossy())?;

    // Build initial state with detected agents.
    let agents = detect_agents();
    let mut state = AppState {
        model: cfg.model.clone(),
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
