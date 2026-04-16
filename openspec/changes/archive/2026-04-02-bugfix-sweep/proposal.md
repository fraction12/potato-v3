## Why

Potato has accumulated bugs and code quality issues across its first sprint of feature work. Several are HIGH severity — concurrency hazards in PTY/MCP, data integrity gaps in session storage, integer overflow in tick counters, and type confusion between pane IDs and indices. Additionally, duplicated code and fragile patterns make future work riskier. A focused sweep now prevents these from compounding as we add more panes and agents.

## What Changes

**Bug fixes (12 open tickets):**
- T-858: Add log rotation detection — reset offset when file shrinks
- T-862: Introduce `PaneId` newtype to eliminate index/ID confusion in injection API
- T-863: Remove double raw-mode setup — let ratatui own terminal mode exclusively
- T-864: Wire up dirty_rx so PTY dirty notifications actually drive redraws
- T-868: Add kill signal to PTY stderr reader task so it shuts down on pane close
- T-869: Validate keybind config strings at load time, reject malformed bindings
- T-874: Tag TextDone events with turn ID to prevent wrong-turn overwrites
- T-875: Remap orphaned session rows when SessionBound replaces session_id
- T-880: Consolidate duplicate `truncate_str` into single `util::truncate_str`
- T-881: Fix `compact_json` to use char-safe truncation instead of byte slicing
- T-884: Handle MCP tool_result array content blocks per spec
- T-886: Deduplicate keybind entries in help overlay

**New bugs (from audit):**
- N-1: `rename_session` disables FK enforcement globally — use transaction-scoped `defer_foreign_keys`
- N-2: `openspec_refresh_ticks` is `u16` — overflows/panics in debug after ~54min; change to `u32`
- N-3: `handle_tool_call` acquires MCP state lock 3× per call — consolidate to single acquisition
- N-4: `validate_structured_message` uses fragile `unwrap()` on sentinel Options — restructure to eliminate panic risk
- N-5: `AppState::default()` creates dead channels (receiver dropped) — affects test correctness

**Code quality:**
- Remove remaining `truncate_str` duplicates
- Consolidate byte-vs-char truncation patterns across the codebase

## Capabilities

### New Capabilities
_(none — this is a fix/quality pass, not new features)_

### Modified Capabilities
- `structured-messaging`: T-884 changes tool_result parsing to handle array content blocks per MCP spec

## Impact

- **src/pty/**: dirty notification wiring (T-864), stderr kill signal (T-868)
- **src/mcp/**: triple-lock consolidation (N-3), structured message validation (N-4), tool_result parsing (T-884)
- **src/mcp/injection.rs**: PaneId newtype (T-862)
- **src/session/store.rs**: FK pragma fix (N-1), session remap (T-875)
- **src/app/state.rs**: tick counter type (N-2), default channels (N-5), dirty_rx wiring (T-864)
- **src/main.rs**: terminal guard cleanup (T-863), turn-tagged events (T-874)
- **src/claude_log.rs + src/codex_log.rs**: rotation detection (T-858), truncation fix (T-881)
- **src/config/**: keybind validation (T-869)
- **src/ui/overlays/help.rs**: dedup bindings (T-886)
- **src/util.rs**: single truncate_str (T-880)
