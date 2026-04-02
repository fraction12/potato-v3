//! Real PTY integration — spawns a process inside a true pseudo-terminal and
//! feeds its output through a [`vt100::Parser`] so the rendered screen can be
//! displayed by [`tui_term::widget::PseudoTerminal`].
//!
//! # Design
//!
//! ```text
//!  ┌──────────┐    raw bytes    ┌────────────────┐   process()   ┌────────────────┐
//!  │ PTY slave │ ──────────────▶ │ reader thread  │ ────────────▶ │ vt100::Parser  │
//!  │ (claude) │                 │ (std::thread)  │               │ (Arc<Mutex<…>) │
//!  └──────────┘                 └────────────────┘               └────────────────┘
//!                                       │ dirty_tx.send(())               │
//!                                       ▼                                 │
//!                                 broadcast::Sender                       │
//!                                   (UI re-render)                        │
//!                                                                         ▼
//!  ┌──────────────────┐   write_input()   ┌──────────────┐     PseudoTerminal widget
//!  │ Potato key event │ ────────────────▶ │ master writer│
//!  └──────────────────┘                   └──────────────┘
//! ```
//!
//! The old [`super::PtyProcess::spawn_turn`] path is preserved — this module
//! adds a *parallel* real-PTY path used when interactive mode is active.

use std::io::Read;
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use portable_pty::{CommandBuilder, PtySize, native_pty_system};
use tokio::sync::broadcast;

// ── RealPty ───────────────────────────────────────────────────────────────────

/// A live pseudo-terminal session wrapping an arbitrary binary (typically
/// `claude`).  All output is fed into a [`vt100::Parser`] that tracks the
/// current terminal screen state; the shared [`Arc<Mutex<vt100::Parser>>`]
/// can be handed to [`tui_term::widget::PseudoTerminal`] for rendering.
///
/// `RealPty` does not derive `Debug` automatically because it contains
/// boxed trait objects (`dyn MasterPty`, `dyn Child`, `dyn Write`).
/// A manual impl is provided that produces a placeholder string.
pub struct RealPty {
    /// VT100 parser — shared between the background reader thread and the
    /// ratatui renderer.  Lock, call `.screen()`, render, drop the guard.
    pub screen: Arc<Mutex<vt100::Parser>>,

    /// Write raw bytes to the child process's stdin (keyboard input).
    ///
    /// Call [`RealPty::write_input`] rather than accessing this directly so
    /// that flush is always performed.
    pub writer: Box<dyn std::io::Write + Send>,

    /// Subscribe to this channel to know when new PTY output has been
    /// processed and the screen needs re-rendering.
    pub dirty_tx: broadcast::Sender<()>,

    /// The initial broadcast receiver, kept alive so no notifications are lost
    /// between spawn and forwarding setup.  Use [`Self::take_dirty_rx`] to claim it.
    dirty_rx: Option<broadcast::Receiver<()>>,

    /// Set to `true` by the reader thread when the child process exits (EOF).
    exited: Arc<std::sync::atomic::AtomicBool>,

    // NOTE: We keep `_child` alive so the process is not reaped on drop.
    // There is no field for the PtyPair because `openpty` consumes it and
    // we extract the master/slave handles before storing them.
    _child: Box<dyn portable_pty::Child + Send + Sync>,

    /// Master handle — needed for `resize()`.
    master: Box<dyn portable_pty::MasterPty + Send>,
}

impl std::fmt::Debug for RealPty {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RealPty")
            .field("screen", &"Arc<Mutex<vt100::Parser>>")
            .field("writer", &"Box<dyn Write + Send>")
            .field("dirty_tx", &self.dirty_tx)
            .finish_non_exhaustive()
    }
}

impl RealPty {
    /// Spawn `binary` with `args` inside a real pseudo-terminal of size
    /// `cols × rows`.
    ///
    /// A background OS thread is spawned that reads PTY output, feeds
    /// [`vt100::Parser`], and notifies `dirty_tx` receivers so the UI can
    /// schedule a re-render.  The thread exits cleanly when the child process
    /// closes its end of the PTY (EOF on the master).
    ///
    /// # Errors
    ///
    /// Returns an error if the PTY cannot be opened, the writer cannot be
    /// taken, or the reader cannot be cloned (all OS-level operations).
    pub fn spawn(binary: &str, args: &[&str], cols: u16, rows: u16) -> Result<Self> {
        Self::spawn_in(binary, args, cols, rows, None)
    }

    /// Like `spawn` but sets the child process's working directory to `cwd`.
    pub fn spawn_in(
        binary: &str,
        args: &[&str],
        cols: u16,
        rows: u16,
        cwd: Option<&std::path::Path>,
    ) -> Result<Self> {
        Self::spawn_with_env(binary, args, cols, rows, cwd, &[])
    }

    /// Like `spawn_in` but also sets additional environment variables for
    /// the child process.
    ///
    /// Each element of `env` is an `(key, value)` pair.  These are applied
    /// *after* the child inherits the parent environment, so they override any
    /// existing value for the same key.
    pub fn spawn_with_env(
        binary: &str,
        args: &[&str],
        cols: u16,
        rows: u16,
        cwd: Option<&std::path::Path>,
        env: &[(String, String)],
    ) -> Result<Self> {
        let pty_system = native_pty_system();

        // Open the PTY pair at the requested size.
        let pair = pty_system
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .context("failed to open PTY pair")?;

        // Build the child command.
        let mut cmd = CommandBuilder::new(binary);
        for arg in args {
            cmd.arg(arg);
        }
        if let Some(dir) = cwd {
            cmd.cwd(dir);
        }
        for (k, v) in env {
            cmd.env(k, v);
        }

        // Spawn the child on the slave side.
        let child = pair
            .slave
            .spawn_command(cmd)
            .context("failed to spawn command in PTY")?;

        // Take the writer (stdin to the child) from the master.
        // NOTE: take_writer() can only be called once — do it before
        // try_clone_reader() to avoid ordering issues.
        let writer = pair
            .master
            .take_writer()
            .context("failed to take PTY master writer")?;

        // Clone a reader handle for the background thread.
        let mut reader = pair
            .master
            .try_clone_reader()
            .context("failed to clone PTY master reader")?;

        // Shared vt100 parser — renderer and reader share this via Arc<Mutex>.
        let screen = Arc::new(Mutex::new(vt100::Parser::new(rows, cols, 10_000)));

        // Dirty-notify channel: capacity 64 is enough — the UI coalesces ticks.
        let (dirty_tx, dirty_rx) = broadcast::channel::<()>(64);

        let exited = Arc::new(std::sync::atomic::AtomicBool::new(false));

        // ── Background reader thread ──────────────────────────────────────────
        // Must be a real OS thread (not tokio task) because the portable-pty
        // reader uses blocking `std::io::Read`.
        {
            let screen_clone = screen.clone();
            let dirty_clone = dirty_tx.clone();
            let exited_clone = exited.clone();

            std::thread::Builder::new()
                .name("pty-reader".to_string())
                .spawn(move || {
                    let mut buf = [0u8; 4096];
                    loop {
                        match reader.read(&mut buf) {
                            Ok(0) | Err(_) => {
                                // EOF or error — child exited.
                                exited_clone.store(true, std::sync::atomic::Ordering::Relaxed);
                                let _ = dirty_clone.send(());
                                break;
                            }
                            Ok(n) => {
                                // Feed the raw bytes into the VT100 parser.
                                if let Ok(mut p) = screen_clone.lock() {
                                    p.process(&buf[..n]);
                                }
                                // Notify the UI; ignore send errors (no subscribers
                                // means no UI is attached — that's fine).
                                let _ = dirty_clone.send(());
                            }
                        }
                    }
                })
                .context("failed to spawn PTY reader thread")?;
        }

        Ok(Self {
            screen,
            writer,
            dirty_tx,
            dirty_rx: Some(dirty_rx),
            exited,
            _child: child,
            master: pair.master,
        })
    }

    /// Resize the PTY to match the new chat area dimensions.
    ///
    /// Call this whenever Potato's layout changes and the chat area rect
    /// changes size.
    pub fn resize(&self, cols: u16, rows: u16) -> Result<()> {
        self.master
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .context("failed to resize PTY")?;
        // Also resize the vt100 parser so it tracks the correct screen size.
        if let Ok(mut p) = self.screen.lock() {
            p.screen_mut().set_size(rows, cols);
        }
        Ok(())
    }

    /// Set the scrollback buffer size (in rows) and return the actual size applied.
    pub fn set_scrollback(&self, rows: usize) -> usize {
        if let Ok(mut p) = self.screen.lock() {
            p.screen_mut().set_scrollback(rows);
            p.screen().scrollback()
        } else {
            0
        }
    }

    /// Return the current scrollback buffer size (in rows).
    pub fn scrollback(&self) -> usize {
        if let Ok(p) = self.screen.lock() {
            p.screen().scrollback()
        } else {
            0
        }
    }

    /// Returns `true` if the PTY child process has exited.
    pub fn child_exited(&self) -> bool {
        self.exited.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Write raw terminal bytes to the child's stdin.
    ///
    /// Call this with the output of [`key_event_to_bytes`] to forward
    /// keyboard events to the running process.
    pub fn write_input(&mut self, bytes: &[u8]) -> Result<()> {
        use std::io::Write;
        self.writer
            .write_all(bytes)
            .context("failed to write PTY input")?;
        self.writer.flush().context("failed to flush PTY input")?;
        Ok(())
    }

    /// Take the initial dirty notification receiver.  This receiver has been
    /// alive since spawn, so no notifications are lost between spawn and
    /// forwarding setup.  Returns `None` if already taken.
    ///
    /// Prefer this over [`Self::subscribe_dirty`] for the primary forwarding task.
    pub fn take_dirty_rx(&mut self) -> Option<broadcast::Receiver<()>> {
        self.dirty_rx.take()
    }

    /// Subscribe to dirty notifications.  The receiver fires whenever the
    /// vt100 parser has consumed new output and the screen should be
    /// re-rendered.
    ///
    /// Note: messages sent before this call are lost.  Use [`Self::take_dirty_rx`]
    /// for the primary consumer to avoid a gap.
    pub fn subscribe_dirty(&self) -> broadcast::Receiver<()> {
        self.dirty_tx.subscribe()
    }
}

// ── key_event_to_bytes ────────────────────────────────────────────────────────

/// Convert a crossterm `KeyEvent` to the raw terminal byte sequence that
/// should be written to the PTY's stdin.
///
/// This is a free function so it can be unit-tested without constructing a
/// [`RealPty`].
///
/// # Mapping
///
/// | Key          | Bytes         |
/// |--------------|---------------|
/// | Printable    | UTF-8         |
/// | Enter        | `\r`          |
/// | Backspace    | `\x7f`        |
/// | Tab          | `\t`          |
/// | Escape       | `\x1b`        |
/// | Arrow Up     | `\x1b[A`      |
/// | Arrow Down   | `\x1b[B`      |
/// | Arrow Right  | `\x1b[C`      |
/// | Arrow Left   | `\x1b[D`      |
/// | Ctrl+C       | `\x03`        |
/// | Ctrl+D       | `\x04`        |
/// | Ctrl+Z       | `\x1a`        |
/// | Ctrl+L       | `\x0c`        |
/// | Ctrl+U       | `\x15`        |
/// | Ctrl+W       | `\x17`        |
/// | F1–F12       | standard ANSI |
pub fn key_event_to_bytes(event: crossterm::event::KeyEvent) -> Vec<u8> {
    use crossterm::event::{KeyCode, KeyModifiers};

    let ctrl = event.modifiers.contains(KeyModifiers::CONTROL);

    match event.code {
        // ── Printable characters ─────────────────────────────────────────
        KeyCode::Char(c) if ctrl => {
            // Ctrl+letter: map to control character (1–26).
            let lower = c.to_ascii_lowercase();
            if lower.is_ascii_alphabetic() {
                vec![lower as u8 - b'a' + 1]
            } else {
                // Ctrl+non-alpha: pass through as-is.
                let mut buf = [0u8; 4];
                c.encode_utf8(&mut buf).as_bytes().to_vec()
            }
        }
        KeyCode::Char(c) => {
            let mut buf = [0u8; 4];
            c.encode_utf8(&mut buf).as_bytes().to_vec()
        }

        // ── Special keys ─────────────────────────────────────────────────
        KeyCode::Enter => b"\r".to_vec(),
        KeyCode::Backspace => b"\x7f".to_vec(),
        KeyCode::Tab => b"\t".to_vec(),
        KeyCode::BackTab => b"\x1b[Z".to_vec(),
        KeyCode::Esc => b"\x1b".to_vec(),
        KeyCode::Delete => b"\x1b[3~".to_vec(),
        KeyCode::Home => b"\x1b[H".to_vec(),
        KeyCode::End => b"\x1b[F".to_vec(),
        KeyCode::PageUp => b"\x1b[5~".to_vec(),
        KeyCode::PageDown => b"\x1b[6~".to_vec(),
        KeyCode::Insert => b"\x1b[2~".to_vec(),

        // ── Arrow keys ────────────────────────────────────────────────────
        KeyCode::Up => b"\x1b[A".to_vec(),
        KeyCode::Down => b"\x1b[B".to_vec(),
        KeyCode::Right => b"\x1b[C".to_vec(),
        KeyCode::Left => b"\x1b[D".to_vec(),

        // ── Function keys (F1–F12) ────────────────────────────────────────
        KeyCode::F(1) => b"\x1bOP".to_vec(),
        KeyCode::F(2) => b"\x1bOQ".to_vec(),
        KeyCode::F(3) => b"\x1bOR".to_vec(),
        KeyCode::F(4) => b"\x1bOS".to_vec(),
        KeyCode::F(5) => b"\x1b[15~".to_vec(),
        KeyCode::F(6) => b"\x1b[17~".to_vec(),
        KeyCode::F(7) => b"\x1b[18~".to_vec(),
        KeyCode::F(8) => b"\x1b[19~".to_vec(),
        KeyCode::F(9) => b"\x1b[20~".to_vec(),
        KeyCode::F(10) => b"\x1b[21~".to_vec(),
        KeyCode::F(11) => b"\x1b[23~".to_vec(),
        KeyCode::F(12) => b"\x1b[24~".to_vec(),

        // ── Null / unknown ────────────────────────────────────────────────
        KeyCode::Null => b"\x00".to_vec(),

        _ => vec![],
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use std::time::Duration;

    // ── Helper: build a plain KeyEvent (no modifiers) ─────────────────────

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn ctrl_key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::CONTROL)
    }

    // ── key_event_to_bytes tests ──────────────────────────────────────────

    #[test]
    fn key_event_to_bytes_enter() {
        assert_eq!(key_event_to_bytes(key(KeyCode::Enter)), b"\r");
    }

    #[test]
    fn key_event_to_bytes_backspace() {
        assert_eq!(key_event_to_bytes(key(KeyCode::Backspace)), b"\x7f");
    }

    #[test]
    fn key_event_to_bytes_tab() {
        assert_eq!(key_event_to_bytes(key(KeyCode::Tab)), b"\t");
    }

    #[test]
    fn key_event_to_bytes_escape() {
        assert_eq!(key_event_to_bytes(key(KeyCode::Esc)), b"\x1b");
    }

    #[test]
    fn key_event_to_bytes_arrow_up() {
        assert_eq!(key_event_to_bytes(key(KeyCode::Up)), b"\x1b[A");
    }

    #[test]
    fn key_event_to_bytes_arrow_down() {
        assert_eq!(key_event_to_bytes(key(KeyCode::Down)), b"\x1b[B");
    }

    #[test]
    fn key_event_to_bytes_arrow_right() {
        assert_eq!(key_event_to_bytes(key(KeyCode::Right)), b"\x1b[C");
    }

    #[test]
    fn key_event_to_bytes_arrow_left() {
        assert_eq!(key_event_to_bytes(key(KeyCode::Left)), b"\x1b[D");
    }

    #[test]
    fn key_event_to_bytes_ctrl_c() {
        assert_eq!(key_event_to_bytes(ctrl_key(KeyCode::Char('c'))), b"\x03");
    }

    #[test]
    fn key_event_to_bytes_ctrl_d() {
        assert_eq!(key_event_to_bytes(ctrl_key(KeyCode::Char('d'))), b"\x04");
    }

    #[test]
    fn key_event_to_bytes_ctrl_z() {
        assert_eq!(key_event_to_bytes(ctrl_key(KeyCode::Char('z'))), &[0x1a]);
    }

    #[test]
    fn key_event_to_bytes_printable_char() {
        assert_eq!(key_event_to_bytes(key(KeyCode::Char('a'))), b"a");
        assert_eq!(key_event_to_bytes(key(KeyCode::Char('Z'))), b"Z");
        assert_eq!(key_event_to_bytes(key(KeyCode::Char(' '))), b" ");
    }

    #[test]
    fn key_event_to_bytes_unicode_char() {
        // Unicode multibyte char should be encoded correctly.
        let bytes = key_event_to_bytes(key(KeyCode::Char('é')));
        assert_eq!(bytes, "é".as_bytes());
    }

    #[test]
    fn key_event_to_bytes_f1_f4() {
        assert_eq!(key_event_to_bytes(key(KeyCode::F(1))), b"\x1bOP");
        assert_eq!(key_event_to_bytes(key(KeyCode::F(2))), b"\x1bOQ");
        assert_eq!(key_event_to_bytes(key(KeyCode::F(3))), b"\x1bOR");
        assert_eq!(key_event_to_bytes(key(KeyCode::F(4))), b"\x1bOS");
    }

    #[test]
    fn key_event_to_bytes_f5_f12() {
        assert_eq!(key_event_to_bytes(key(KeyCode::F(5))), b"\x1b[15~");
        assert_eq!(key_event_to_bytes(key(KeyCode::F(10))), b"\x1b[21~");
        assert_eq!(key_event_to_bytes(key(KeyCode::F(12))), b"\x1b[24~");
    }

    #[test]
    fn key_event_to_bytes_home_end_page() {
        assert_eq!(key_event_to_bytes(key(KeyCode::Home)), b"\x1b[H");
        assert_eq!(key_event_to_bytes(key(KeyCode::End)), b"\x1b[F");
        assert_eq!(key_event_to_bytes(key(KeyCode::PageUp)), b"\x1b[5~");
        assert_eq!(key_event_to_bytes(key(KeyCode::PageDown)), b"\x1b[6~");
    }

    // ── RealPty integration tests (require a real TTY — marked #[ignore]) ─

    /// Spawn `echo hello` in a PTY and verify that the vt100 parser received
    /// the output bytes (non-empty screen content after a brief wait).
    ///
    /// This test is `#[ignore]` because it requires an OS-level TTY and
    /// spawning a real process.  Run with `cargo test -- --ignored` on a
    /// machine where that is safe.
    #[test]
    #[ignore = "requires real TTY / OS process"]
    fn real_pty_spawn_echo() {
        let pty =
            RealPty::spawn("sh", &["-c", "echo hello"], 80, 24).expect("RealPty::spawn failed");

        // Give the background reader thread time to process the output.
        std::thread::sleep(Duration::from_millis(200));

        // The vt100 parser should have received the "hello" bytes.
        let parser = pty.screen.lock().unwrap();
        let screen = parser.screen();
        let mut contents = String::new();
        for row in 0..24u16 {
            for col in 0..80u16 {
                if let Some(cell) = screen.cell(row, col) {
                    let s = cell.contents();
                    if !s.is_empty() {
                        contents.push_str(s);
                    }
                }
            }
        }

        assert!(
            contents.contains("hello"),
            "expected 'hello' in PTY screen contents; got: {contents:?}",
        );
    }

    /// Spawn `cat` in a PTY, write "hello\n", and verify the echo appears in
    /// the vt100 parser's screen.
    ///
    /// `#[ignore]` — requires real TTY.
    #[test]
    #[ignore = "requires real TTY / OS process"]
    fn real_pty_write_input() {
        let mut pty = RealPty::spawn("cat", &[], 80, 24).expect("RealPty::spawn failed");

        // Write input to the PTY.
        pty.write_input(b"hello\r").expect("write_input failed");

        // Give the reader thread time to process the echo.
        std::thread::sleep(Duration::from_millis(200));

        let parser = pty.screen.lock().unwrap();
        let screen = parser.screen();
        let mut contents = String::new();
        for row in 0..24u16 {
            for col in 0..80u16 {
                if let Some(cell) = screen.cell(row, col) {
                    let s = cell.contents();
                    if !s.is_empty() {
                        contents.push_str(s);
                    }
                }
            }
        }

        assert!(
            contents.contains("hello"),
            "expected 'hello' echoed back in PTY screen; got: {contents:?}",
        );
    }
}
