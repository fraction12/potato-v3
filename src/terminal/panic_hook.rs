//! Panic hook — handles panics differently depending on which thread panics.
//!
//! **Main thread panic:** restore terminal and exit (classic behaviour).
//! **Background thread panic:** set a flag so the event loop forces a full
//! TUI redraw on the next tick.  The app keeps running — the panic is logged
//! (via stderr redirect to the log file) and the TUI self-heals.

use std::panic;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;

use crossterm::{
    execute,
    terminal::{LeaveAlternateScreen, disable_raw_mode},
};

/// Global flag: set by the panic hook on background-thread panics.
/// The event loop checks this each tick and calls `terminal.clear()`.
static REDRAW_NEEDED: AtomicBool = AtomicBool::new(false);

/// Check (and clear) whether a background-thread panic requests a full redraw.
pub fn take_redraw_flag() -> bool {
    REDRAW_NEEDED.swap(false, Ordering::SeqCst)
}

/// Install the panic hook.
///
/// - Main thread: restore terminal + print panic (classic).
/// - Background thread: set the redraw flag, log the panic, keep running.
pub fn install_panic_hook() {
    let main_thread_id = thread::current().id();
    let original = panic::take_hook();

    panic::set_hook(Box::new(move |info| {
        if thread::current().id() == main_thread_id {
            // Main thread — classic restore-and-die behaviour.
            let _ = disable_raw_mode();
            let _ = execute!(std::io::stderr(), LeaveAlternateScreen);
            original(info);
        } else {
            // Background thread — flag a redraw, log the panic.
            // stderr is already redirected to the log file (T-906),
            // so this write goes to disk, not the TUI surface.
            REDRAW_NEEDED.store(true, Ordering::SeqCst);
            eprintln!(
                "[potato] background thread panic (TUI redraw scheduled): {info}"
            );
        }
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn take_redraw_flag_returns_false_when_not_set() {
        REDRAW_NEEDED.store(false, Ordering::SeqCst);
        assert!(!take_redraw_flag());
    }

    #[test]
    fn take_redraw_flag_clears_after_read() {
        REDRAW_NEEDED.store(true, Ordering::SeqCst);
        assert!(take_redraw_flag());
        // Second read should be false — flag was cleared.
        assert!(!take_redraw_flag());
    }

    #[test]
    fn simulated_background_panic_sets_and_clears_flag() {
        // Simulate what the panic hook does on a background thread:
        // set the flag, then verify the event loop can take and clear it.
        REDRAW_NEEDED.store(false, Ordering::SeqCst);

        // Simulate: background thread sets the flag.
        let handle = std::thread::spawn(|| {
            REDRAW_NEEDED.store(true, Ordering::SeqCst);
        });
        handle.join().unwrap();

        assert!(
            take_redraw_flag(),
            "flag should be set after background thread write"
        );
        assert!(
            !take_redraw_flag(),
            "flag should be cleared after take"
        );
    }
}
