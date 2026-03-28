# Changelog

## Unreleased

### Added (Phase 8 — Multi-Agent Support)
- **CodexAdapter** (T-801) — Full `AgentAdapter` implementation for the Codex CLI. Parses JSONL events: `thread.started` → `SessionBound`, `turn.completed` → `TurnDone` (with cached+input token merging), `item.started/item.completed` → `ToolStart`/`ToolDone`/`TextDone`. Interactive PTY mode (no `--print` needed). `detect()` searches PATH + `/opt/homebrew/bin/codex`. 36 unit tests. `codex resume <id>` support via `AdapterConfig.resume_session_id`.
- **CodexSessionLogTracker** (T-801) — Incremental JSONL tracker for `~/.codex/sessions/YYYY/MM/DD/*.jsonl`. Parses `session_meta`, `response_item` (title, turns), `event_msg` (usage, item_started/completed). `find_session_log()` searches all date dirs. 15 unit tests.
- **Agent profiles system** (T-804) — `AgentProfile` struct with name/adapter/binary/model/extra_args/env/working_dir. `ProfileLoader::load()` merges: auto-detected defaults < `~/.config/potato/profiles/*.toml` < `.potato/profile.toml`. Supports single-profile and `[[profiles]]` array TOML format. 11 unit tests.
- **Agent picker overlay** (T-803) — `/agent` command opens centered overlay listing Claude/Codex/OpenCode with ●/○ availability indicators, binary path, and capabilities (SRAT: Structured/Resumable/Approval/Tools). Up/Down/Enter/Esc navigation. Enter launches selected agent via `spawn_agent_pane()`.
- **`spawn_agent_pane()`** (T-803) — Generalizes `spawn_claude_pane()` to support `claude`, `codex`, and any generic adapter by adapter name. Dispatches to correct binary detection and PTY spawn path.
- **Dynamic left rail agents section** — Shows all 3 detected agents (Claude Code, Codex, OpenCode) with ●/○ status indicators instead of hardcoded "+ Claude".
- **`detect_agents()`** extended — Now includes OpenCode via `GenericAdapter` alongside Claude and Codex.

### Changed (Phase 8)
- `/agent` command now shows `AgentPicker` overlay instead of `Handled` no-op.
- `OverlayKind::AgentPicker` added to command registry.
- `Overlay::AgentPicker` added to `AppState` overlay enum.
- `AgentPickerState { selected }` added to `SessionState`.
- `current_phase` advanced to `phase-8-multiagent` in spec.yaml.
- Test count: **576 → 653** (+77 new tests).



### Added
- **MCP foundation layer** (T-700) — 113-test TDD implementation across 5 modules: protocol types, inter-session state, tool definitions+dispatch, MCP server handler, and .mcp.json config writer. All types, state management, and handler logic ready for wiring.
- **MCP spike validated** (T-700b) — Confirmed Claude Code discovers and uses project-scoped .mcp.json servers. Tool calls work with `bypassPermissions`. JSONL logs capture MCP tool results.
- **Inter-session communication spec** — Full architecture for MCP-based agent-to-agent coordination (Phase 7). Potato runs an MCP server that both Claude panes connect to, enabling real-time messaging, shared context, task coordination, and role assignment. PTY injection provides push notifications between sessions. See `INTER-SESSION.md` in Second Brain docs.
- **Multi-pane Tab cycling** — Tab naturally cycles through all panes when focus is on Terminal. Active pane shown with ● indicator.
- **Dashboard→pane spawn fix** — Dashboard Enter now correctly uses PaneManager instead of legacy singleton path.

### Fixed
- **Flash-to-dashboard bug** — Dashboard Enter used legacy `real_pty` spawn path instead of `spawn_claude_pane()`, causing dead-pane detector to immediately bounce back to dashboard.
- **macOS keybind conflict** — Removed Ctrl+Arrow pane switching (conflicts with macOS Spaces/window management).

### Changed
- Phase numbering updated: Inter-Session Communication is Phase 7, Multi-Agent Support moved to Phase 8, Polish+Distribution moved to Phase 9.
