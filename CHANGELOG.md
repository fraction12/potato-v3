# Changelog

## Unreleased

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
