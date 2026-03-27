//! Panic hook — restores the terminal to a sane state on panic.

use std::panic;

use crossterm::{
    execute,
    terminal::{LeaveAlternateScreen, disable_raw_mode},
};

/// Install a panic hook that restores the terminal before printing the panic.
///
/// Without this, a panic while in raw/alternate-screen mode leaves the user's
/// terminal in an unusable state.
pub fn install_panic_hook() {
    let original = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        // Best-effort terminal restore — ignore errors since we're panicking.
        let _ = disable_raw_mode();
        let _ = execute!(std::io::stderr(), LeaveAlternateScreen);
        original(info);
    }));
}
