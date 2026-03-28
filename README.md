# 🥔 Potato

**A terminal cockpit for coding agents.**

Potato doesn't replace your agents — it gives them a home. Spawn Claude Code, Codex, or any CLI agent inside real embedded terminals, then watch them work side by side with live observability and built-in coordination.

> *Think of it as mission control for the agents doing your actual coding.*

[![Rust](https://img.shields.io/badge/Rust-1.86+-orange?logo=rust)](https://www.rust-lang.org/)
[![Tests](https://img.shields.io/badge/tests-663_passing-brightgreen)]()
[![Status](https://img.shields.io/badge/status-alpha-yellow)]()
[![License](https://img.shields.io/badge/license-MIT-blue)](LICENSE)

---

## Why Potato?

You already have great coding agents. What you don't have is a way to **run two of them on the same project**, see what they're both doing, and let them talk to each other.

Potato fixes that:

- 🖥️ **Real terminals** — agents run in actual PTYs, not simulated chat. Every keystroke, every tool call, exactly as if you ran them directly.
- 📊 **Live observability** — token usage, tool calls, model info, and session metrics parsed from agent logs in real time.
- 🤝 **Agent coordination** — built-in MCP server lets agents send messages, share context, and claim tasks through Potato.
- 🔀 **Side-by-side panes** — run an architect and an implementer simultaneously on the same codebase.
- 📜 **Session history** — browse, resume, and export past sessions. Never lose context.

## Quick Start

```bash
# Build from source
git clone https://github.com/nicholasengleman/potato.git
cd potato
cargo build --release

# Launch in your project directory
cd ~/my-project
potato
```

**Requirements:**
- Rust 1.86+ (edition 2024)
- At least one supported agent: [Claude Code](https://code.claude.com), [Codex](https://github.com/openai/codex), or any CLI tool

## The Cockpit

```
┌─────────┬───────────────────────────────┬───────────┐
│ Agents  │                               │           │
│ ● Claude│   Embedded Agent Terminal     │ Coordin.  │
│ ○ Codex │   (real PTY — not a sim)      │ ────────  │
│         │                               │ ◉ Arch.   │
│─────────│                               │ ◎ Impl.   │
│ Sessions│                               │           │
│ ▸ refac.│                               │ Activity  │
│   fix l.│                               │ ────────  │
│   add t.│                               │ ↔ msg...  │
│         ├───────────────────────────────┤ ✓ task..  │
│         │ > describe the auth flow_     │           │
├─────────┴───────────────────────────────┴───────────┤
│ ● Claude · sonnet-4 · Idle · 12.4k tokens  [Input] │
└─────────────────────────────────────────────────────┘
```

**Left rail** — Agent launcher + session history  
**Center** — Real embedded terminal (supports scrollback, mouse, full TUI)  
**Right rail** — Coordination status + activity feed + metrics  
**Bottom** — Input bar + status  

## Supported Agents

| Agent | Adapter | Live Metrics | Resume | Notes |
|-------|---------|:---:|:---:|-------|
| [Claude Code](https://code.claude.com) | `claude` | ✅ | ✅ | First-class. Full JSONL log parsing. |
| [Codex](https://github.com/openai/codex) | `codex` | ✅ | ✅ | Interactive PTY mode. |
| Any CLI | `generic` | — | — | Wraps any terminal command as an agent. |

## Multi-Agent Coordination

Run two agents side by side and let them collaborate through Potato's MCP layer:

```
┌──────────────────┐    MCP     ┌──────────────────┐
│   Claude (Arch)  │◄──tools───►│     Potato       │◄──tools───►│  Claude (Impl)  │
│                  │            │  coordination    │            │                 │
│ potato_send_msg  │            │  ┌────────────┐  │            │ potato_send_msg │
│ potato_get_role  │            │  │ messages   │  │            │ potato_get_role │
│ potato_claim_task│            │  │ tasks      │  │            │ potato_claim_task│
│ potato_shared_ctx│            │  │ context    │  │            │ potato_shared_ctx│
└──────────────────┘            │  │ roles      │  │            └─────────────────┘
                                │  └────────────┘  │
                                └──────────────────┘
```

1. `/role architect` and `/role implementer` — assign responsibilities
2. Agents discover each other's roles and status via MCP tools
3. `potato_send_message` — direct messaging between agents, injected into PTY
4. `potato_shared_context` — shared key-value store for decisions and specs
5. `potato_claim_task` / `potato_release_task` — mutex-like task ownership

Agents coordinate through Potato, not through ad-hoc file passing.

## Keyboard Shortcuts

| Key | Context | Action |
|-----|---------|--------|
| `Tab` / `Shift+Tab` | Global | Cycle focus: Agents → Sessions → Input → Terminal → Sidebar |
| `Ctrl+J` | Any | Jump to terminal pane |
| `Ctrl+Q` | Terminal | Return to input |
| `Ctrl+W` | Any | Close active pane |
| `Ctrl+\` | Any | Quit Potato |
| `Esc` | Input | Clear input buffer |
| `?` | Any | Toggle help overlay |
| `/` | Input | Enter command mode |
| `PgUp` / `PgDn` | Terminal | Scroll terminal independently |
| `End` | Terminal | Jump to live bottom |
| `Alt+[` / `Alt+]` | Any | Switch between panes |

## Slash Commands

| Command | Description |
|---------|-------------|
| `/new` | New agent session |
| `/agent` | Agent picker overlay |
| `/role <name> [desc]` | Assign role to current pane |
| `/help` | Keybind reference |
| `/sessions` | Browse session history |
| `/export` | Export current session |

## Architecture

```
┌─────────────────────────────────────────┐
│             TUI (ratatui)               │
│   Dashboard · Session · Overlays        │
├─────────────────────────────────────────┤
│        App State (Elm-style)            │
│   Pure reducer · Panes · Focus · Metrics│
├──────────┬────────────┬─────────────────┤
│ Adapters │  PTY Layer │   MCP Server    │
│ Claude   │  spawn     │   UDS bridge    │
│ Codex    │  read/write│   tools/state   │
│ Generic  │  resize    │   injection     │
├──────────┴────────────┴─────────────────┤
│   Session Store (SQLite WAL)            │
│   Claude/Codex JSONL Log Trackers       │
└─────────────────────────────────────────┘
```

**Design principles:**
- **Agents own conversation truth** — Potato reads agent artifacts, never invents state
- **PTY-first** — real terminals, not simulated chat
- **Elm-style state** — pure reducer, side-effect-free rendering
- **Truth-first observability** — metrics from agent log files, not estimates

## Configuration

```toml
# ~/.config/potato/config.toml

[default]
adapter = "claude"
model = "claude-sonnet-4-20250514"
```

Project-level overrides in `.potato/profile.toml`:

```toml
[[profiles]]
name = "architect"
adapter = "claude"
extra_args = ["--permission-mode", "bypassPermissions"]

[[profiles]]
name = "implementer"
adapter = "claude"
extra_args = ["--permission-mode", "bypassPermissions"]
```

## Development

```bash
# Run the test suite (663 tests)
cargo test

# Run with debug tracing
RUST_LOG=debug cargo run

# Release build
cargo build --release
```

**Stats:** ~25k lines of Rust across 66 modules. 663 tests passing.

## Roadmap

- [x] Real PTY embedding with Claude Code + Codex
- [x] Side-by-side multi-pane cockpit
- [x] Live observability from agent JSONL logs
- [x] Inter-agent MCP coordination (messaging, tasks, shared context)
- [x] Session history, resume, and discovery
- [x] Agent profiles and multi-adapter support
- [ ] Coordination observatory sidebar
- [ ] Per-project persistent state (`.potato/`)
- [ ] Project-scoped agent roster
- [ ] `cargo install` + Homebrew distribution

## License

[MIT](LICENSE)

---

<sub>Built with [ratatui](https://github.com/ratatui/ratatui) · [portable-pty](https://github.com/wez/wezterm/tree/main/pty) · [vt100](https://github.com/doy/vt100-rust) · [tui-term](https://github.com/a-kenji/tui-term)</sub>
