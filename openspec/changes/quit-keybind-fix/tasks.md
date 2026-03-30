# Tasks — quit-keybind-fix

## Summary

`Ctrl+\` is documented as the quit binding everywhere but doesn't actually work — terminals intercept it as SIGQUIT before crossterm can surface it as a key event. `Ctrl+Q` already works and is wired up. Standardize on `Ctrl+Q` everywhere.

## Bug

`Ctrl+\` sends SIGQUIT (signal 3) on Unix terminals. The raw mode terminal captures most control sequences, but `Ctrl+\` is often handled at the OS/driver level before reaching the application. crossterm never delivers it as a `KeyEvent`. The binding was aspirational — it was in all the help text and footers but never actually intercepted in `handle_key()`.

Meanwhile `Ctrl+Q` and `Ctrl+C` are both properly wired in `src/app/update.rs` and work fine.

## Fix

Replace all `Ctrl+\` references with `Ctrl+Q`. No handler changes needed — just UI text updates.

## Tasks

- [ ] T-1030: [BUG] Update help overlay — replace `Ctrl+\` with `Ctrl+Q` in both normal and terminal-focus key entries. File: `src/ui/overlays/help.rs`
- [ ] T-1031: [BUG] Update session screen footer bars — replace `Ctrl+\\` with `Ctrl+Q` in status bar text (both multi-pane and single-pane variants). File: `src/ui/screens/session.rs`
- [ ] T-1032: [BUG] Update dashboard footer and doc comment — replace `Ctrl+\` / `Ctrl+\\` with `Ctrl+Q` in footer text and module doc comment. File: `src/ui/screens/dashboard.rs`
- [ ] T-1033: [BUG] Update keybinds config default — if `keybinds.rs` default quit binding references backslash, change to `ctrl+q`. Update/add test asserting `Ctrl+Q` is the documented default. File: `src/config/keybinds.rs`
- [ ] T-1034: [BUG] Update session screen doc comment — replace `Ctrl+\` reference in terminal-focus key interception docs. File: `src/ui/screens/session.rs`

## Files Affected

- `src/ui/overlays/help.rs` — help text
- `src/ui/screens/session.rs` — footer bars + doc comment
- `src/ui/screens/dashboard.rs` — footer text + module doc
- `src/config/keybinds.rs` — default binding + tests
