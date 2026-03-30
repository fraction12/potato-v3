## 1. Snapshot module

- [x] 1.1 Create `src/openspec/snapshot.rs` with `OpenSpecSnapshot`, `ChangeInfo`, `ArtifactInfo` structs (all `Debug, Clone, Default`)
- [x] 1.2 Implement `OpenSpecSnapshot::capture()` — detect `openspec` binary, run `openspec list --json`, parse into `Vec<ChangeInfo>`
- [x] 1.3 Implement per-change `openspec status --change <name> --json` calls (capped at 5 most recent in-progress changes) to populate `ArtifactInfo` on each `ChangeInfo`
- [x] 1.4 Add helper `openspec_output(args)` function (same pattern as `git_output` in `src/git.rs`)
- [x] 1.5 Add unit tests for snapshot: empty when CLI missing, parsing valid JSON, partial failure resilience

## 2. Main loop integration

- [x] 2.1 Add `openspec_snapshot: OpenSpecSnapshot` field to `AppState` (replace `openspec: Option<OpenSpecWatcher>`)
- [x] 2.2 Add `openspec_refresh_ticks: u16` counter to `AppState`
- [x] 2.3 Wire up periodic refresh in main loop (~30s / 120 ticks) calling `OpenSpecSnapshot::capture()`
- [x] 2.4 Wire up F5 manual refresh to also recapture the OpenSpec snapshot
- [x] 2.5 Seed initial snapshot at startup (call `capture()` once before entering the loop)

## 3. UI panel update

- [x] 3.1 Rename sidebar block title from `" Tasks "` to `" OpenSpec "` in `src/ui/screens/session.rs`
- [x] 3.2 Replace task-list rendering with per-change summary lines: change name + task progress (e.g., "2/7") + artifact completion indicator
- [x] 3.3 Update empty-state messages: "openspec not found" when CLI missing, "no changes" when snapshot is empty but CLI exists
- [x] 3.4 Update adaptive layout height calculation to use change count instead of task count

## 4. MCP compatibility

- [x] 4.1 Update `InterSessionState.openspec_tasks` population to derive from `OpenSpecSnapshot` changes instead of the old watcher
- [x] 4.2 Update `sync_openspec()` in `main.rs` to work with the new snapshot (or remove it if tick-based refresh replaces it)
- [x] 4.3 Verify `potato_list_tasks` MCP tool still returns compatible data

## 5. Cleanup

- [x] 5.1 Remove `src/openspec/parser.rs` (markdown parser)
- [x] 5.2 Remove `src/openspec/watcher.rs` (file watcher)
- [x] 5.3 Remove `notify` / `PollWatcher` imports and usage from openspec module
- [x] 5.4 Update `src/openspec/mod.rs` to export `snapshot` module instead of `parser` + `watcher`
- [x] 5.5 Run `cargo test` and fix any broken references across the codebase
