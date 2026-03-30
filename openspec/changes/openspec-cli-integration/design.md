## Context

Potato currently uses a homegrown markdown parser + `notify::PollWatcher` to read OpenSpec task data from `openspec/changes/*/tasks.md` files. This approach only extracts checkbox-level task data (id, title, open/done) and misses the richer data the OpenSpec CLI provides: change-level metadata, artifact completion status, task counts, and timestamps.

Meanwhile, `src/git.rs` already demonstrates a clean pattern for CLI integration: a `GitSnapshot::capture()` method shells out to CLI commands, parses output, and returns a snapshot struct that gets stored in `AppState` and periodically refreshed in the main loop.

The OpenSpec CLI (`openspec` v1.2.0) supports `--json` on all major commands, making it straightforward to adopt the same pattern.

## Goals / Non-Goals

**Goals:**
- Replace file-based parsing with `openspec` CLI calls (`openspec list --json`, `openspec status --change <name> --json`)
- Follow the `GitSnapshot` pattern: synchronous `capture()`, periodic refresh, non-fatal on failure
- Display richer data in the sidebar: per-change name, task progress (completed/total), artifact status, last modified
- Rename the sidebar panel from "Tasks" to "OpenSpec"
- Keep MCP `potato_list_tasks` tool working by deriving data from the new snapshot

**Non-Goals:**
- Writing back to OpenSpec (no `openspec` write commands) — Potato remains read-only
- Replacing the MCP task claims system — that's orthogonal coordination state
- Real-time streaming from OpenSpec — periodic polling is sufficient

## Decisions

### 1. CLI snapshot over file parsing

**Decision:** Shell out to `openspec list --json` and `openspec status --change <name> --json` instead of parsing markdown files.

**Why:** The CLI is the canonical interface. It handles format changes, validation, and edge cases internally. Our parser duplicates that logic and will drift. JSON output gives us richer data (completion %, artifact graphs, timestamps) for free.

**Alternative considered:** Keep file parsing but add CLI calls for enrichment. Rejected — maintaining two data sources adds complexity with no benefit.

### 2. Two-command capture strategy

**Decision:** `capture()` runs `openspec list --json` first, then `openspec status --change <name> --json` for each in-progress change (capped at 5 most recent).

**Why:** `list` gives us the change-level overview. `status` per-change gives artifact detail. Capping at 5 keeps capture fast — most projects don't have more than a few active changes.

**Alternative considered:** Single `list` call only. Rejected — artifact status is valuable for the UI and worth the extra calls.

### 3. Periodic refresh matching git pattern

**Decision:** Refresh every ~30 seconds (120 ticks at 250ms) in the main loop, same as `GitSnapshot`. Also refresh on F5 user trigger.

**Why:** OpenSpec data changes infrequently (when agents write artifacts or complete tasks). 30s is responsive enough. Matches existing git refresh cadence, keeping the architecture consistent.

**Alternative considered:** Keep the file watcher approach alongside CLI. Rejected — adding CLI already gives us fresher data on each snapshot, and removing the watcher simplifies the architecture.

### 4. Flat struct, not nested watcher

**Decision:** `OpenSpecSnapshot` is a plain `#[derive(Debug, Clone, Default)]` struct (like `GitSnapshot`), stored directly on `AppState`. No `Arc<Mutex<>>`, no background threads, no channel.

**Why:** The git pattern proves this is sufficient. `capture()` runs synchronously and fast enough (~50-100ms for a few CLI calls). The main loop calls it periodically. Simpler ownership, no locking.

### 5. Panel rename and layout

**Decision:** Rename sidebar block title from `" Tasks "` to `" OpenSpec "`. Show a summary line per change (name + progress bar/fraction) instead of individual task lines.

**Why:** The panel now represents OpenSpec status broadly, not just tasks. Per-change summaries are more useful at a glance than a flat task list that could be dozens of items.

## Risks / Trade-offs

**[Risk] CLI not installed** → `capture()` returns `OpenSpecSnapshot::default()` (empty). UI shows "openspec not found" message. Same non-fatal pattern as git.

**[Risk] CLI is slow or hangs** → Use `Command::output()` which is blocking but bounded by the OS process timeout. Could add explicit timeout via `std::process::Command` + `wait_with_output` if needed. For v1, acceptable since `openspec` commands return in <100ms.

**[Risk] Breaking JSON schema changes in future OpenSpec versions** → We parse with `serde_json::Value` (not strongly-typed deserialize) for resilience, same as `git.rs` does with `gh` output. If a field is missing, we default gracefully.

**[Trade-off] Losing real-time file watch** → We trade 2-second file-change detection for 30-second CLI polling. Acceptable because: (a) agents don't need sub-second task updates, (b) F5 gives instant refresh, (c) the CLI data is richer.
