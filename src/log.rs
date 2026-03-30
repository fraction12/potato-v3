//! File-based logging for Potato.
//!
//! Writes structured tracing output to `~/.potato/potato.log` so that
//! Potato's own diagnostics are never mixed with the terminal it hands off
//! to Claude Code or another agent.
//!
//! [`redirect_stderr`] redirects fd 2 to the same log file so that *any*
//! write to stderr (eprintln!, panic output, library debug spew) goes to
//! disk instead of corrupting the ratatui surface.

use std::fs::OpenOptions;
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};

use tracing_subscriber::{EnvFilter, fmt, prelude::*};

/// Return the canonical log-file path (`~/.potato/potato.log`).
#[must_use]
pub fn log_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_default()
        .join(".potato")
        .join("potato.log")
}

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
    let path = log_path();
    std::fs::create_dir_all(path.parent().unwrap())?;
    let file = OpenOptions::new().create(true).append(true).open(&path)?;
    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("debug")))
        .with(
            fmt::layer()
                .with_writer(move || file.try_clone().unwrap())
                .with_ansi(false),
        )
        .init();
    Ok(())
}

/// Redirect the process stderr (fd 2) to `target_path`.
///
/// After this call every `eprintln!`, panic message, or library debug write
/// goes to disk instead of the terminal surface.  This is the primary
/// defence against background-thread output corrupting the ratatui TUI.
///
/// # Safety
/// Uses `libc::dup2` which is safe when `target_path` opens successfully.
pub fn redirect_stderr(target_path: &Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(target_path.parent().unwrap())?;
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(target_path)?;
    let fd = file.as_raw_fd();
    // dup2 atomically replaces fd 2 with a copy of `fd`.
    let ret = unsafe { libc::dup2(fd, 2) };
    if ret == -1 {
        anyhow::bail!("dup2 failed: {}", std::io::Error::last_os_error());
    }
    // `file` is intentionally leaked — fd must stay open for the process lifetime.
    std::mem::forget(file);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    #[test]
    fn log_path_ends_with_potato_log() {
        let p = log_path();
        assert!(p.ends_with(".potato/potato.log"));
    }

    #[test]
    fn redirect_stderr_captures_eprintln() {
        use std::io::Write;

        // Write to a temp file via stderr redirect, then verify contents.
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("test-stderr.log");

        redirect_stderr(&target).unwrap();

        // Write directly to fd 2 via libc to avoid Rust buffering races.
        let msg = b"potato-stderr-test-marker\n";
        unsafe {
            libc::write(2, msg.as_ptr() as *const libc::c_void, msg.len());
        }

        let mut contents = String::new();
        std::fs::File::open(&target)
            .unwrap()
            .read_to_string(&mut contents)
            .unwrap();
        assert!(
            contents.contains("potato-stderr-test-marker"),
            "stderr was not redirected to file; contents: {contents:?}"
        );
    }
}
