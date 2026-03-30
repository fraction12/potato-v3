## Why

The tasks panel currently uses a homegrown markdown parser (`src/openspec/parser.rs`) that reads `tasks.md` files directly from disk via a file watcher. This is fragile (breaks if OpenSpec's format evolves), limited (only gets checkbox status, no artifact/change-level data), and inconsistent with how we handle git (CLI-based snapshots). The OpenSpec CLI v1.2.0 provides `--json` output on all commands, giving us richer, more reliable data. We should integrate with the CLI the same way `src/git.rs` integrates with `git` — shell out, parse JSON, store a snapshot.

## What Changes

- Replace the file-based `OpenSpecWatcher` + `OpenSpecBacklog` parser with a new `OpenSpecSnapshot` struct that shells out to the `openspec` CLI
- Call `openspec list --json` to get all changes with task counts, completion %, status, and last modified timestamps
- Call `openspec status --change <name> --json` per active change to get artifact dependency graphs (proposal/specs/design/tasks status)
- Refresh periodically in the main loop (same tick-based pattern as `GitSnapshot`)
- Rename the sidebar "Tasks" section to "OpenSpec" and display richer data: change names, artifact completion, task counts, phase status
- Keep MCP `potato_list_tasks` working by deriving task data from the new snapshot
- Remove the `notify` file watcher dependency for openspec (no longer needed)

## Capabilities

### New Capabilities
- `openspec-snapshot`: CLI-based snapshot capture for OpenSpec data (changes, artifacts, task counts) — replaces file-based parsing with `openspec list --json` and `openspec status --change <name> --json`

### Modified Capabilities
<!-- No existing spec-level requirements are changing -->

## Impact

- **Code removed:** `src/openspec/parser.rs` (markdown parser), `src/openspec/watcher.rs` (file watcher), `notify`/`PollWatcher` dependency for openspec
- **Code added/modified:** New `src/openspec/snapshot.rs` (CLI snapshot), updated `src/openspec/mod.rs`, updated `src/main.rs` (swap watcher for snapshot refresh), updated `src/ui/screens/session.rs` (richer panel rendering, rename to "OpenSpec"), updated `src/mcp/tools.rs` + `src/mcp/state.rs` (derive task list from snapshot)
- **Dependencies:** No new crates needed — uses `std::process::Command` + `serde_json` (both already in the project)
- **UI:** Sidebar panel renamed from "Tasks" to "OpenSpec", shows per-change artifact status and task progress instead of flat task list
