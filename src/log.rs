//! File-based logging for Potato.
//!
//! Writes structured tracing output to `~/.potato/potato.log` so that
//! Potato's own diagnostics are never mixed with the terminal it hands off
//! to Claude Code or another agent.

use std::fs::OpenOptions;

use tracing_subscriber::{fmt, EnvFilter, prelude::*};

/// Initialise file-based logging.
///
/// All tracing output is written to `~/.potato/potato.log`.  The log level
/// is controlled by the `RUST_LOG` environment variable (defaults to
/// `debug` when unset).
///
/// Calling this more than once in a process will panic (tracing-subscriber
/// only allows one global subscriber).  Call it once, at the very start of
/// `main()`.
pub fn init_file_logging() -> anyhow::Result<()> {
    let log_path = dirs::home_dir()
        .unwrap_or_default()
        .join(".potato")
        .join("potato.log");
    std::fs::create_dir_all(log_path.parent().unwrap())?;
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)?;
    tracing_subscriber::registry()
        .with(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("debug")),
        )
        .with(
            fmt::layer()
                .with_writer(move || file.try_clone().unwrap())
                .with_ansi(false),
        )
        .init();
    eprintln!("Logging to: {}", log_path.display());
    Ok(())
}
