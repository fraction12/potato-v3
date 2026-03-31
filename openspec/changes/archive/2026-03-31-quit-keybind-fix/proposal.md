## Why

`Ctrl+\` is documented as the quit binding in help text, footers, and keybind defaults, but it never works — terminals intercept it as SIGQUIT (signal 3) before crossterm can deliver it as a key event. The actual quit handler in `update.rs` is hardcoded to `Ctrl+Q` / `Ctrl+C`. All user-facing text needs to match what actually works.

## What Changes

- **BREAKING**: Default quit keybind in `keybinds.rs` changes from `ctrl+\` to `ctrl+q`
- Update help overlay entries to show `Ctrl+Q` instead of `Ctrl+\`
- Update dashboard footer hints to show `Ctrl+Q`
- Update session footer status bars to show `Ctrl+Q`
- Update doc comments referencing `Ctrl+\`
- Fix test assertions to expect `ctrl+q`

## Capabilities

### New Capabilities
<!-- None — this is a documentation/config bug fix -->

### Modified Capabilities
<!-- None — no existing spec-level behavior changes, only fixing UI text to match reality -->

## Impact

- `src/ui/overlays/help.rs` — help text (2 entries)
- `src/ui/screens/session.rs` — footer bars (2 lines) + doc comment
- `src/ui/screens/dashboard.rs` — footer hints (2 lines) + test assertion
- `src/config/keybinds.rs` — default binding + test
- No handler changes — `Ctrl+Q` and `Ctrl+C` already work in `src/app/update.rs`
