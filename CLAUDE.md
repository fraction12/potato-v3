# CLAUDE.md — Potato V3

## What is Potato?

Terminal cockpit for coding agents. Spawns Claude Code, Codex, or any CLI agent in a real PTY and wraps it in a unified TUI with observability, session management, and inter-agent coordination via MCP.

Potato is **not** an agent — it's iTerm2 for the agent era. No LLM API calls, no tool execution. It spawns real agent processes and observes them.

## Quick Reference

```bash
cargo check          # type-check (do this first, it's fast)
cargo test           # ~789 tests, runs in <1s
cargo fmt --check    # formatting check
cargo run            # launch the TUI (boots to dashboard)
cargo run -- mcp-server  # run as MCP server (used by spawned agents)
```

## Language & Stack

- **Language:** Rust (edition 2024, MSRV 1.86)
- **TUI:** ratatui + crossterm
- **Async:** tokio (full features)
- **PTY:** portable-pty + vt100 + tui-term
- **Storage:** SQLite via rusqlite (WAL mode)
- **Config:** TOML via `toml` crate
- **CLI:** clap (derive)

## Architecture

### Core Mental Model

```
Dashboard (agent picker) → Session (cockpit with embedded PTY terminals)
```

The app has two screens: `AppScreen::Dashboard` and `AppScreen::Session`. Dashboard is the launch pad; Session hosts live agent PTYs with a 3-column layout (left rail | terminal + input | sidebar).

### Module Map

| Module | Purpose |
|---|---|
| `src/main.rs` | Entry point, event loop, agent detection, pane spawning |
| `src/app/state.rs` | `AppState` — all global state, screen transitions |
| `src/app/pane.rs` | `PaneManager` — manages up to 2 side-by-side PTY panes |
| `src/app/update.rs` | State mutation handlers |
| `src/adapters/` | `AgentAdapter` trait + Claude, Codex, Generic implementations |
| `src/config/` | Config loading (`~/.potato/config.toml`), profiles, keybinds, schema |
| `src/config/profiles.rs` | `ProfileLoader` — merges auto-detected + global + project agent profiles |
| `src/pty/` | PTY process spawning and I/O (real.rs) |
| `src/mcp/` | Built-in MCP server for inter-agent coordination |
| `src/claude_log.rs` | Claude JSONL session log tailer for observability |
| `src/codex_log.rs` | Codex log tailer |
| `src/ui/screens/` | Dashboard and Session screen renderers |
| `src/ui/panels/` | Sidebar panels (chat, agent status, token dashboard, etc.) |
| `src/ui/overlays/` | Modal overlays (agent picker, help, model picker, confirm) |
| `src/ui/focus/` | Focus ring system (Tab/Shift+Tab/Ctrl+J/Esc) |
| `src/ui/layout/` | Layout manager and presets |
| `src/ui/theme.rs` | Earth-tone color palette |
| `src/input/` | Input handlers per screen/focus context |
| `src/session/` | SQLite session persistence, history, export |
| `src/roles.rs` | `.potato/roles.toml` loading for pane role assignments |
| `src/openspec/` | OpenSpec file watcher + parser for backlog/spec display |
| `src/events/` | Event stream (crossterm → tokio channel) |
| `src/metrics/` | Session metrics aggregation |
| `src/git.rs` | Git status capture for dashboard |

### Key Design Decisions

- **Potato spawns real agents in a PTY** — no LLM API calls, no tool execution
- **Dashboard-first launch** → Enter to start a cockpit session
- **Focus model:** Input default → Tab cycles → Ctrl+J terminal → Esc returns
- **Inter-session comms via MCP server** + PTY injection for push notifications
- **Potato is its own MCP server binary** (`potato mcp-server` subcommand)
- **Sidebar metrics from Claude's native JSONL session logs**, not synthetic estimates
- **Profile loading priority:** auto-detected defaults < `~/.config/potato/profiles/*.toml` < `.potato/agents.toml`

### Agent Adapter System

The `AgentAdapter` trait (`src/adapters/mod.rs`) defines how each agent is detected, launched, and parsed:

```rust
trait AgentAdapter: Send + Sync {
    fn name(&self) -> &str;
    fn detect(&self) -> Option<PathBuf>;          // find binary
    fn capabilities(&self) -> AdapterCapabilities; // S/R/A/T flags
    fn build_command(&self, config: &AdapterConfig) -> Command;
    fn parse_line(&self, line: &str) -> Vec<AgentEvent>;
    fn format_user_input(&self, text: &str) -> String;
    fn format_approval(&self, approved: bool) -> Option<String>;
}
```

Three adapters: `ClaudeAdapter`, `CodexAdapter`, `GenericAdapter`.

### MCP Coordination

Potato runs an MCP server over a Unix domain socket. Each agent pane connects to it automatically via `.mcp.json`. MCP tools:

- `potato_claim_role` / `potato_get_role` — role assignment
- `potato_send_message` / `potato_get_messages` — inter-agent messaging
- `potato_shared_context` — KV store
- `potato_claim_task` / `potato_release_task` / `potato_list_tasks` — task coordination

State persists to `.potato/state.db` (SQLite, WAL mode).

## Project Files

| Path | Purpose |
|---|---|
| `~/.potato/config.toml` | Global config (default agent, DB path, keybinds, theme) |
| `~/.config/potato/profiles/*.toml` | Global agent profiles |
| `.potato/agents.toml` | Project-scoped agent roster (overrides global) |
| `.potato/roles.toml` | Team role definitions for this project |
| `.potato/state.db` | Per-project coordination state (roles, messages, tasks) |
| `.mcp.json` | Auto-generated MCP config for agent discovery (do not edit) |

## Conventions

### Code Style
- `#[must_use]` on pure functions returning values
- Doc comments (`///`) on all public items
- Module-level doc comments (`//!`) at top of each file
- Section headers with `// ── Section Name ──────` comment bars
- Tests in `#[cfg(test)] mod tests` at the bottom of each file
- Use `thiserror` for error enums, `anyhow` at the top level

### State Management
- All mutable state lives in `AppState` (`src/app/state.rs`)
- Screen-specific state in `DashboardState` or `SessionState`
- Cross-screen state (like `agent_profiles`, `store`, `mcp_socket_path`) on `AppState` directly
- Panes managed by `PaneManager` (max 2 concurrent)

### Input Handling
- Input routed by screen and focus context (`src/input/`)
- `dashboard.rs` handles dashboard keys, `session.rs` handles session keys
- Terminal-focused input goes to `terminal.rs` (raw PTY passthrough)
- `text_input.rs` for the input bar widget

### Testing
- Tests are co-located in each module, not in a separate `tests/` dir
- Use `tempfile` for filesystem tests
- Tests should be fast — no network, no real PTY spawning
- Current: ~789 tests, all passing in <1s

## Development Workflow — OpenSpec

This project uses **OpenSpec** (`openspec` CLI at `/opt/homebrew/bin/openspec`) for spec-driven development. All planning, task tracking, and implementation flow through OpenSpec.

### Key Skills (invoke via `/skill`)
- `/openspec` — main entry point, manages proposal → specs → design → tasks → implementation
- `/openspec-propose` — propose a new change (generates design, specs, and tasks in one step)
- `/openspec-apply-change` — implement tasks from an existing change
- `/openspec-explore` — thinking partner for exploring ideas before committing to a change
- `/openspec-archive-change` — archive a completed change

### Finding Current Work
- Run `openspec status` to see the current phase, open tickets, and backlog
- Run `openspec list` to see all changes and their status
- Do **not** read `.openspec/` YAML files directly — use the CLI or skills

## Common Tasks

### Adding a new adapter
1. Create `src/adapters/my_agent.rs` implementing `AgentAdapter`
2. Add detection in `detect_agents()` in `main.rs`
3. Add to agent picker in `src/ui/overlays/agent_picker.rs`

### Adding a new sidebar panel
1. Create `src/ui/panels/my_panel.rs` implementing the panel
2. Register in `src/ui/panels/mod.rs` (`PanelId` enum)
3. Add to layout in `src/ui/layout/mod.rs`

### Adding a new MCP tool
1. Define handler in `src/mcp/tools.rs`
2. Register in tool list in `src/mcp/server.rs`
3. Add state methods in `src/mcp/state.rs` if needed

### Adding a new overlay
1. Create `src/ui/overlays/my_overlay.rs`
2. Add overlay state to `AppState` or `SessionState`
3. Handle input in the relevant `src/input/` handler
4. Render in the screen's draw function

## Gotchas

- `main.rs` has `#![allow(dead_code, unused_imports, unused_variables)]` — this is intentional during scaffold phase, not a mistake
- `.mcp.json` is auto-generated on pane spawn — don't commit manual edits
- The PTY reader runs on background threads; panics there are caught by the panic hook in `src/terminal/panic_hook.rs` and trigger a TUI redraw flag
- `ProfileLoader::load()` uses `std::env::current_dir()` to find `.potato/agents.toml` — make sure CWD is the project root
- Claude JSONL log paths depend on project directory naming (underscores → dashes normalization in `claude_log.rs`)
