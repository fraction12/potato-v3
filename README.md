# 🥔 Potato

**The terminal cockpit for coding agents.**

Potato spawns agents. Agents do the work. Potato makes the work observable.

> *Think iTerm2 for the agent era — not another agent, but the place where agents live.*

![Rust](https://img.shields.io/badge/Rust-1.86+-orange?logo=rust)
![Status](https://img.shields.io/badge/status-alpha-yellow)
![License](https://img.shields.io/badge/license-MIT-blue)

---

## What is this?

Potato is a native terminal application (Rust + [ratatui](https://github.com/ratatui/ratatui)) that embeds real coding agents — Claude Code, Codex, and others — inside managed PTY sessions. Instead of replacing your agents, Potato wraps them in a cockpit that adds:

- **Real embedded terminals** — agents run in actual PTYs, not simulated chat
- **Side-by-side panes** — run two agents simultaneously on the same project
- **Live observability** — token usage, tool calls, session metrics parsed from agent logs
- **Inter-session communication** — agents can talk to each other via MCP tools
- **Session history** — browse and resume past sessions from a persistent rail
- **Slash commands** — `/new`, `/role`, `/help`, `/agent`, and more

## Quick Start

```bash
# Clone and build
git clone https://github.com/fraction12/potato-v3.git
cd potato-v3
cargo build --release

# Run (launches dashboard, pick an agent)
./target/release/potato

# Or specify working directory
./target/release/potato --workdir ~/my-project
```

**Requirements:**
- Rust 1.86+ (edition 2024)
- At least one supported agent installed: [Claude Code](https://code.claude.com), [Codex](https://github.com/openai/codex), or any CLI tool (generic adapter)

## Cockpit Layout

```
┌─────────┬──────────────────────────────┬──────────┐
│ Agents  │                              │ Claude   │
│ + Claude│     Embedded Agent PTY       │ ──────── │
│ + Codex │     (real terminal)          │ Model    │
│─────────│                              │ Tokens   │
│ Sessions│                              │ Tools    │
│ > chat..│                              │ Totals   │
│   fix.. │                              │          │
│         │                              │          │
│         ├──────────────────────────────┤          │
│         │ > your input here_           │          │
├─────────┴──────────────────────────────┴──────────┤
│ [Input] Tab:focus  ?:help  /:command    1 pane    │
└───────────────────────────────────────────────────┘
```

## Keyboard Shortcuts

| Key | Action |
|-----|--------|
| `Tab` / `Shift+Tab` | Cycle focus: Agents → Sessions → Input → Terminal → Sidebar |
| `Ctrl+J` | Focus terminal pane |
| `Esc` | Return to input / close pane |
| `?` | Toggle help overlay |
| `/` | Enter command mode |
| `PgUp` / `PgDn` | Scroll terminal (when focused) |
| `End` | Jump to bottom of terminal |
| `Enter` | Send input to agent / execute command |
| `q` / `Ctrl+C` | Quit |

## Slash Commands

| Command | Alias | Description |
|---------|-------|-------------|
| `/new` | `/n` | New agent session |
| `/agent` | `/a` | Agent picker overlay |
| `/sessions` | `/s` | Session picker |
| `/role <name> [desc]` | `/r` | Set pane role (for multi-agent coordination) |
| `/help` | `/h`, `/?` | Keyboard shortcuts |
| `/export` | `/e` | Export current session |

## Supported Agents

| Agent | Adapter | Metrics | Resume | Notes |
|-------|---------|---------|--------|-------|
| **Claude Code** | `claude` | ✅ Full (JSONL parsing) | ✅ `--resume` | First-class. Sidebar shows tokens, tools, model. |
| **Codex** | `codex` | ✅ Full (JSONL parsing) | ✅ `resume <id>` | Interactive PTY mode. |
| **Any CLI** | `generic` | ❌ Raw output only | ❌ | Wraps any terminal command. |

## Multi-Agent Coordination

Potato can run two agents side-by-side with built-in coordination:

1. **MCP Server** — Potato runs as an MCP server for each pane, giving agents tools like `send_message`, `get_partner_status`, `shared_context`, and `claim_task`
2. **PTY Injection** — Potato pushes notifications directly into agent terminals
3. **Role Assignment** — `/role architect` and `/role engineer` let agents know their responsibilities
4. **Shared State** — Message queues, key-value context, and task boards mediated by Potato

Agents coordinate through Potato, not through ad-hoc file passing.

## Architecture

```
┌─────────────────────────────────────┐
│            TUI (ratatui)            │
│  Dashboard │ Session │ Overlays     │
├─────────────────────────────────────┤
│          App State (Elm-style)      │
│  Reducer │ Panes │ Focus │ Metrics  │
├─────────┬───────────┬───────────────┤
│ Adapters│ PTY Layer │  MCP Server   │
│ Claude  │ spawn     │  UDS bridge   │
│ Codex   │ read/write│  tools/state  │
│ Generic │ resize    │  injection    │
├─────────┴───────────┴───────────────┤
│        Session Store (SQLite)       │
│     Claude/Codex Log Trackers       │
└─────────────────────────────────────┘
```

**Key design principles:**
- Agents are the source of truth for conversation identity
- Potato is the source of truth for observability
- PTY-first: agents run in real terminals, not simulated
- Elm-style state: pure reducer + side-effect-free rendering
- Truth-first: sidebar metrics come from agent log files, not estimates

## Project Structure

```
src/
├── main.rs              — Entry, CLI, event loop, PTY lifecycle
├── adapters/            — Agent adapter trait + Claude/Codex/Generic
├── app/                 — State, panes, reducer, messages, actions
├── claude_log.rs        — Claude JSONL session log parser/tracker
├── codex_log.rs         — Codex JSONL session log parser/tracker
├── commands/            — Slash command registry and parsing
├── config/              — Schema, keybinds, profiles
├── events/              — Unified AgentEvent enum
├── mcp/                 — MCP server, UDS bridge, tools, state, injection
├── metrics/             — Token/cost accumulation
├── pty/                 — PTY spawning, reading, env wiring
├── session/             — SQLite store, discovery, export, history
├── terminal/            — Event stream, panic hook
└── ui/
    ├── screens/         — Dashboard, session cockpit
    ├── overlays/        — Help, agent picker, slash menu, confirm
    ├── panels/          — Chat, tool output, file preview, sessions
    ├── widgets/         — Sparkline, tool card, status badge
    └── theme.rs         — Earth-tone WCAG-compliant palette
```

## Configuration

Global config: `~/.config/potato/config.toml`
Project config: `.potato/profile.toml`

```toml
[default]
adapter = "claude"
model = "claude-sonnet-4-20250514"

[profiles.fast]
adapter = "codex"
model = "gpt-5.4"
extra_args = ["--full-auto"]
```

## Development

```bash
# Run tests (576+ and counting)
cargo test

# Run with tracing
RUST_LOG=debug cargo run

# Check without building
cargo check
```

## Status

**Alpha** — actively developed. The core cockpit works: you can spawn Claude/Codex sessions, see live metrics, resume history, run side-by-side panes, and coordinate via MCP. Polish and distribution (Homebrew, `cargo install`) coming in Phase 9.

## License

MIT
