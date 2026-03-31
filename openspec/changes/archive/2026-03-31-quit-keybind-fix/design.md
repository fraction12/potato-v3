## Context

The quit binding has a mismatch: `keybinds.rs` defaults to `ctrl+\`, help/footer UI shows `Ctrl+\`, but the actual handler in `update.rs` is hardcoded to check `Ctrl+Q` and `Ctrl+C`. On Unix terminals, `Ctrl+\` sends SIGQUIT at the OS/driver level before crossterm can intercept it, so the documented binding was never functional.

## Goals / Non-Goals

**Goals:**
- Make all user-facing quit references match the working binding (`Ctrl+Q`)
- Fix the default keybind config to reflect reality

**Non-Goals:**
- Not changing the actual quit handler logic in `update.rs` — it already works
- Not adding SIGQUIT signal handling (not worth the complexity for a keybind alias)
- Not making keybinds configurable/dynamic — that's a separate change (`phase-10-keybind-overhaul`)

## Decisions

### 1. Standardize on `Ctrl+Q`, not attempt to make `Ctrl+\` work

**Choice:** Replace all references rather than adding a SIGQUIT handler.

**Rationale:** Intercepting SIGQUIT requires `signal_hook` or similar, adds platform-specific complexity, and `Ctrl+\` is non-standard for quit. `Ctrl+Q` is conventional and already works. Simplest correct fix.

### 2. Text-only changes, no handler modifications

**Choice:** Only change UI strings, config defaults, doc comments, and tests. No handler logic changes.

**Rationale:** The handler already checks for `Ctrl+Q` and `Ctrl+C`. The bug is purely in documentation/config, not behavior.

## Risks / Trade-offs

- **[Breaking config change]** Users who explicitly set `quit = "ctrl+\\"` in their config will still have a non-functional binding. → **Mitigation:** The binding was never functional, so this changes nothing for them. The default changing to `ctrl+q` is the correct fix.
