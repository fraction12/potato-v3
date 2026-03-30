# Tasks — async-snapshots

## Summary

Move all blocking subprocess calls (`git`, `openspec`, `gh`) off the main event loop into background tasks. The TUI should never freeze waiting on a shell command.

## Problem

`GitSnapshot::capture()` and `OpenspecSnapshot::capture()` use `std::process::Command::output()` — synchronous, blocking calls. They run directly in the main `loop { select! { ... } }` every ~30 seconds. While they execute (100–500ms depending on repo size, disk cache, network for `gh`), the entire TUI is frozen: no redraws, no input handling.

This is the "hang / not snappy" feel. It's not a bug — it's architectural. Every subprocess blocks the event loop.

## Affected call sites in `src/main.rs`

1. **Line ~592**: `state.git_snapshot = git::GitSnapshot::capture();` — blocks on 3+ `git` subprocesses
2. **Line ~583**: `refresh_rail(state, &store);` — may also invoke blocking snapshot calls
3. **Line ~772/940**: `refresh_rail()` called on state transitions — same blocking path

## Design

Use `tokio::task::spawn_blocking` to run captures off the event loop. Results come back via a `tokio::sync::mpsc` channel and get picked up on the next tick. The TUI always has the *last known* snapshot to render — it's never waiting.

```
Main loop                        Background
─────────                        ──────────
tick (every 50ms)
  ├─ check refresh interval
  │   └─ if due → spawn_blocking(capture)
  ├─ poll snapshot_rx channel
  │   └─ if ready → update state
  └─ draw (always uses last-known snapshot)
```

No new threads, no new crates. Just `spawn_blocking` + a channel. Snapshots that arrive mid-frame get picked up on the next 50ms tick — imperceptible delay, zero blocking.

## Tasks

- [ ] T-1020: Add snapshot message channel — create `mpsc::Receiver<SnapshotMsg>` on `AppState` with enum variants for `Git(GitSnapshot)` and `Openspec(OpenspecSnapshot)`. Sender cloned into spawn_blocking closures. File: `src/app/state.rs`, `src/main.rs`
- [ ] T-1021: Make `GitSnapshot::capture()` async-compatible — move the call into `spawn_blocking`, send result on channel. Remove direct `state.git_snapshot = capture()` from main loop. Guard against overlapping refreshes (skip if one is already in-flight). File: `src/main.rs`, `src/git.rs`
- [ ] T-1022: Make `refresh_rail()` non-blocking — any snapshot calls inside `refresh_rail` move to the same spawn_blocking pattern. Rail updates when results arrive via channel, not inline. File: `src/main.rs`
- [ ] T-1023: Drain snapshot channel on tick — poll `snapshot_rx.try_recv()` each tick, update `state.git_snapshot` / `state.openspec_snapshot` / rail data when new results arrive. File: `src/main.rs`
- [ ] T-1024: Prevent overlapping spawns — add `git_refresh_in_flight: bool` and `openspec_refresh_in_flight: bool` to state. Set on spawn, clear on receive. Skip spawn if already in-flight. File: `src/app/state.rs`, `src/main.rs`

## Files Affected

- `src/main.rs` — event loop refactor (primary)
- `src/app/state.rs` — channel receiver + in-flight flags
- `src/git.rs` — no API changes, just called from spawn_blocking now
- `src/openspec/snapshot.rs` — same, no API changes
