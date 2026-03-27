//! Potato — terminal-native AI agent orchestration desktop.
//!
//! Entry point: parse CLI args, initialise tracing, install panic hook,
//! enter raw mode + alternate screen, run the app, restore terminal on exit.

// Scaffold: suppress warnings for types/items not yet wired into the main app.
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
use tracing_subscriber::EnvFilter;

use app::state::AppState;
use terminal::panic_hook::install_panic_hook;

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

// ── App runner ────────────────────────────────────────────────────────────────

/// Build and run the application until the user quits.
fn run(terminal: &mut DefaultTerminal, state: &mut AppState) -> Result<()> {
    loop {
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

        if event::poll(Duration::from_millis(250))? {
            if let Event::Key(key) = event::read()? {
                match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => {
                        state.should_quit = true;
                    }
                    _ => {}
                }
            }
        }

        if state.should_quit {
            break;
        }
    }

    Ok(())
}

// ── Entry point ───────────────────────────────────────────────────────────────

fn main() -> Result<()> {
    // Parse CLI args.
    let cli = Cli::parse();

    // Initialise tracing.
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn")),
        )
        .with_writer(io::stderr)
        .init();

    // Install panic hook so terminal is restored on crash.
    install_panic_hook();

    // Build initial app state.
    let mut state = AppState {
        model: cli.model.unwrap_or_else(|| "llama3".to_string()),
        config_path: cli.config.unwrap_or_default(),
        ..Default::default()
    };

    // Enter raw mode and alternate screen (restored via RAII guard on drop).
    let _guard = TerminalGuard::enter()?;
    let mut terminal = ratatui::init();

    // Run the main loop.
    let result = run(&mut terminal, &mut state);

    // ratatui::restore() is called by DefaultTerminal's Drop, and _guard
    // restores raw mode + alternate screen.
    ratatui::restore();

    result
}
