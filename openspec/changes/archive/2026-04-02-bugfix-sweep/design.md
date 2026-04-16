## Context

Potato V3 has shipped its core feature set (multi-pane PTY, MCP coordination, observability). The codebase has 827 passing tests and compiles clean, but a systematic audit surfaced 12 open bugs from the first sweep plus 5 new issues. Most are concurrency, data integrity, or type-safety problems that get worse as pane count grows.

## Goals / Non-Goals

**Goals:**
- Fix all 17 identified bugs (12 open tickets + 5 new)
- Eliminate duplicate code patterns (truncate_str, byte/char confusion)
- Zero test regressions — all 827+ tests stay green
- Each fix is independently testable and reviewable

**Non-Goals:**
- No new features or UI changes
- No large-scale refactoring beyond what's needed for the fix
- No changes to the adapter trait or MCP protocol surface
- No performance optimization work (beyond fixing the triple-lock)

## Decisions

### D1: PaneId newtype (T-862)
Introduce `pub struct PaneId(pub u64)` in `src/app/pane.rs`. All APIs that currently accept `u64` pane IDs switch to `PaneId`; APIs that accept `usize` indices stay as `usize`. The compiler then catches any index/ID confusion.

*Alternative: just document which is which.* Rejected — the whole point is to make the bug impossible, not just unlikely.

### D2: Transaction-scoped FK handling (N-1)
Replace `PRAGMA foreign_keys = OFF/ON` in `rename_session` with `PRAGMA defer_foreign_keys = ON` inside the transaction. This is transaction-scoped so it can't leak.

*Alternative: wrap in a mutex.* Rejected — adds complexity when SQLite already has the right primitive.

### D3: Consolidate MCP state locks (N-3)
Restructure `handle_tool_call` to acquire the lock once, do timing + dispatch + roster in a single critical section. The individual handler functions take `&mut InterSessionState` instead of `Arc<Mutex<...>>`.

*Alternative: switch to `tokio::sync::Mutex`.* Deferred — that's a larger change. Consolidating first removes the immediate contention.

### D4: Dirty notification wiring (T-864)
In `RealPty::spawn_with_env`, keep the broadcast receiver alive by storing it in the `RealPty` struct. The forwarding task in `main.rs` takes it via `pty.take_dirty_rx()` instead of `subscribe_dirty()`. This ensures no notifications are lost between spawn and forwarding setup.

### D5: Turn-tagged events (T-874)
Add a `turn_id: u64` field to `TextDone` events. The session reducer only applies `TextDone` if `turn_id` matches the current active turn. Stale events are dropped.

### D6: Tick counter overflow (N-2)
Change `openspec_refresh_ticks: u16` → `u32` (matching `git_refresh_ticks`). Use `saturating_add(1)` for both counters.

## Risks / Trade-offs

- **PaneId newtype churn** → Touches many files but is mechanical. Mitigate: do it in one commit, run `cargo check` to catch all callsites.
- **Lock consolidation changes handler signatures** → Individual MCP handlers lose the ability to lock independently. Mitigate: they don't need to — all state access is synchronous and fast.
- **Turn ID requires event schema change** → `AgentEvent::TextDone` gets a new field. Mitigate: default to `0` for backward compat in tests; only match when `> 0`.
- **Test channel fix (N-5)** → Changing `AppState::default()` could break tests that rely on current dead-channel behavior. Mitigate: audit test usage first, fix any tests that depend on send-failure semantics.
