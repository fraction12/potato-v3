//! Potato — terminal-native AI agent orchestration desktop.
//!
//! Entry point: parse CLI args, load config, initialise the session store,
//! register built-in tools, install the panic hook, enter raw mode +
//! alternate screen, run the async event loop, and restore the terminal on exit.

// Scaffold: suppress warnings for types/items not yet fully wired.
#![allow(dead_code, unused_imports)]

mod agent;
mod app;
mod config;
mod ollama;
mod session;
mod terminal;
mod tools;
mod ui;

use std::io::{self, Write as _};
use std::time::Duration;

use anyhow::Result;
use clap::Parser;
use crossterm::{
    event::{self, Event, KeyCode},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    DefaultTerminal,
    layout::Rect,
    text::Line,
    widgets::{Block, Borders, Paragraph},
};
use tokio::sync::mpsc;
use tracing_subscriber::EnvFilter;

use app::message::Message;
use app::state::AppState;
use app::update::update;
use config::load_config;
use session::SessionStore;
use terminal::events::event_stream;
use terminal::panic_hook::install_panic_hook;
use tools::builtin::register_builtins;
use tools::registry::ToolRegistry;

// ── CLI arguments ─────────────────────────────────────────────────────────────

/// Potato — terminal-native AI agent orchestration desktop.
#[derive(Parser, Debug)]
#[command(name = "potato", version, about)]
struct Cli {
    /// LLM model to use (overrides config).
    #[arg(short, long)]
    model: Option<String>,

    /// Path to a custom config file.
    #[arg(short, long)]
    config: Option<String>,
}

// ── RAII terminal guard ───────────────────────────────────────────────────────

/// Restores the terminal to its original state when dropped.
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

// ── Async app loop ────────────────────────────────────────────────────────────

/// Run the async event loop until the user quits.
///
/// - Spawns an event stream task that converts crossterm events into [`Message`]s.
/// - On each tick, re-renders the UI.
/// - On [`Message::Quit`] or the Q key, exits cleanly.
async fn run_async(terminal: &mut DefaultTerminal, state: &mut AppState) -> Result<()> {
    // Kick off the terminal event stream.
    let mut event_rx = event_stream();

    let tick_duration = Duration::from_millis(250);

    loop {
        // Render.
        terminal.draw(|frame| {
            let area = frame.area();
            let block = Block::default()
                .title(" 🥔 Potato v0.1.0 — press q to quit ")
                .borders(Borders::ALL);
            let inner = block.inner(area);
            frame.render_widget(block, area);

            let lines = vec![
                Line::from(""),
                Line::from("  Welcome to Potato — terminal-native AI agent orchestration."),
                Line::from(""),
                Line::from(format!("  Model  : {}", state.model)),
                Line::from("  Status : Idle — scaffold complete, full UI coming soon."),
                Line::from(""),
                Line::from("  Press q to exit."),
            ];
            frame.render_widget(Paragraph::new(lines), inner);
        })?;

        // Wait for either a terminal event or a tick timeout.
        let msg = tokio::select! {
            Some(m) = event_rx.recv() => Some(m),
            _ = tokio::time::sleep(tick_duration) => Some(Message::Tick),
        };

        if let Some(m) = msg {
            let action = update(state, m);
            use app::action::Action;
            if matches!(action, Action::Quit) {
                break;
            }
        }

        if state.should_quit {
            break;
        }
    }

    Ok(())
}

// ── Entry point ───────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<()> {
    // 1. Parse CLI args.
    let cli = Cli::parse();

    // 2. Initialise tracing.
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn")),
        )
        .with_writer(io::stderr)
        .init();

    // 3. Install panic hook so the terminal is restored on crash.
    install_panic_hook();

    // 4. Load configuration.
    let mut config = load_config(cli.config.as_deref())?;

    // CLI model override takes priority over config file.
    if let Some(model) = cli.model {
        config.model = model;
    }

    // 5. Resolve the database path (expand ~) and initialise the session store.
    let db_path = config::expand_tilde(&config.db_path);
    // Ensure the parent directory exists.
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    let db_path_str = db_path.to_string_lossy();
    let _store = SessionStore::open(&db_path_str)?;

    // 6. Register built-in tools.
    let mut registry = ToolRegistry::new();
    register_builtins(&mut registry);
    tracing::info!("registered {} tools", registry.len());

    // 7. Build initial application state from config.
    let mut state = AppState {
        model: config.model.clone(),
        ..Default::default()
    };

    // 8. Enter raw mode and alternate screen (restored via RAII guard on drop).
    let _guard = TerminalGuard::enter()?;
    let mut terminal = ratatui::init();

    // 9. Run the async event loop.
    let result = run_async(&mut terminal, &mut state).await;

    // 10. Restore terminal.
    ratatui::restore();

    result
}
