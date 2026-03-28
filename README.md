# 🥔 Potato

**Give your coding agents teammates.**

Run Claude Code, Codex, or any coding agent side by side in real embedded terminals — and let them collaborate through MCP.

> Potato doesn't replace your agents. It gives them a shared workspace, coordination tools, and a mission control you can actually watch.

[![Rust](https://img.shields.io/badge/Rust-1.86+-orange?logo=rust)](https://www.rust-lang.org/)
[![Tests](https://img.shields.io/badge/tests-663_passing-brightgreen)]()
[![License](https://img.shields.io/badge/license-MIT-blue)](LICENSE)

<!-- TODO: Replace with actual screen recording -->
<!-- ![Potato Demo](docs/assets/demo.gif) -->

---

## The Problem

You have Claude Code. Maybe Codex too. They're great — individually.

But when you want an architect and an implementer working the same codebase? You're alt-tabbing between terminals, copy-pasting context, and playing human router between agents that can't see each other.

## What Potato Does

Potato is a native terminal app (Rust + [ratatui](https://github.com/ratatui/ratatui)) that embeds your real coding agents inside managed PTY sessions and connects them through MCP coordination tools.

**🖥️ Real terminals, not simulation** — Agents run in actual PTYs. Every keystroke, every tool call, exactly as if you ran them directly. No wrappers, no abstractions over their behavior.

**🤝 MCP-native coordination** — Potato runs an MCP server per session. Agents get tools like `send_message`, `claim_task`, `shared_context`, and `get_partner_status` — automatically, without prompting.

**📊 Live observability** — Token usage, tool calls, model info, and session metrics parsed from Claude/Codex JSONL logs in real time. Not estimates — actual agent data.

**🔀 Side-by-side panes** — Two agents, one screen. Assign roles (`/role architect`, `/role implementer`), and they see each other's status and coordinate through Potato.

**📜 Persistent sessions** — Browse and resume past sessions. Pick up where you left off.

## Quick Start

```bash
# Build from source
git clone https://github.com/fraction12/potato-v3.git
cd potato-v3
cargo build --release

# Launch in your project directory
cd ~/my-project
./target/release/potato
```

**Requirements:**
- Rust 1.86+
- At least one agent: [Claude Code](https://docs.anthropic.com/en/docs/claude-code), [Codex](https://github.com/openai/codex), or any CLI tool

## How It Works

```
┌─────────────────────────────────────────────────────┐
│                                                     │
│   ┌─────────────┐            ┌─────────────┐       │
│   │ Claude Code  │◄── MCP ──►│ Claude Code  │       │
│   │ (Architect)  │   tools   │(Implementer) │       │
│   └──────┬───────┘           └──────┬───────┘       │
│          │                          │               │
│          │    ┌──────────────┐      │               │
│          └───►│    Potato     │◄────┘               │
│               │              │                      │
│               │ • messages   │                      │
│               │ • tasks      │                      │
│               │ • context    │                      │
│               │ • roles      │                      │
│               └──────────────┘                      │
│                                                     │
│   Agents talk to each other through Potato.         │
│   You watch it happen.                              │
│                                                     │
└─────────────────────────────────────────────────────┘
```

Each agent gets MCP tools automatically when Potato launches them:

| Tool | What it does |
|------|-------------|
| `potato_send_message` | Send a message to the other agent |
| `potato_get_messages` | Check your inbox |
| `potato_get_partner_status` | See what the other agent is doing |
| `potato_claim_task` | Take ownership of a task |
| `potato_release_task` | Release a task for someone else |
| `potato_shared_context` | Read/write shared key-value state |
| `potato_get_role` | Look up who's who |

No configuration needed. Potato writes the MCP config, injects environment variables, and manages the coordination state.

## The Cockpit

```
┌──────────┬──────────────────────────────┬──────────┐
│ Agents   │                              │          │
│ ● Claude │   Real Embedded Terminal     │ Agents   │
│ ○ Codex  │   (actual PTY output)        │ ● Arch.  │
│          │                              │ ◎ Impl.  │
│──────────│                              │          │
│ Sessions │                              │ Activity │
│ ▸ refac..│                              │ ↔ msg..  │
│   fix l..│                              │ ✓ task.. │
│          ├──────────────────────────────┤          │
│          │ > your prompt here_          │ Metrics  │
├──────────┴──────────────────────────────┴──────────┤
│ ● Claude · sonnet-4 · Idle · 12.4k tokens [Input] │
└───────────────────────────────────────────────────────┘
```

- **Left** — Agent launcher + session history
- **Center** — Real embedded terminal with independent scrollback
- **Right** — Coordination status, activity feed, session metrics
- **Bottom** — Input + status bar

## Supported Agents

| Agent | Adapter | Live Metrics | Session Resume |
|-------|---------|:---:|:---:|
| [Claude Code](https://docs.anthropic.com/en/docs/claude-code) | `claude` | ✅ Full (JSONL) | ✅ |
| [Codex](https://github.com/openai/codex) | `codex` | ✅ Full (JSONL) | ✅ |
| Any CLI | `generic` | — | — |

## Keyboard Shortcuts

| Key | Action |
|-----|--------|
| `Tab` / `Shift+Tab` | Cycle focus across panels |
| `Ctrl+J` | Focus terminal |
| `Ctrl+Q` | Exit terminal focus |
| `Ctrl+W` | Close active pane |
| `Alt+[` / `Alt+]` | Switch between panes |
| `/` | Slash commands |
| `?` | Help overlay |
| `Ctrl+\` | Quit |

## Slash Commands

| Command | Description |
|---------|-------------|
| `/new` | New agent session |
| `/agent` | Agent picker |
| `/role <name>` | Assign role to current pane |
| `/help` | Keyboard shortcuts |

## Architecture

```
┌──────────────────────────────────────────┐
│            TUI (ratatui)                 │
├──────────────────────────────────────────┤
│         App State (Elm-style)            │
│    Pure reducer · Panes · Focus          │
├───────────┬───────────┬──────────────────┤
│  Adapters │ PTY Layer │   MCP Server     │
│  Claude   │ spawn     │   UDS bridge     │
│  Codex    │ read/write│   coordination   │
│  Generic  │ resize    │   tools/state    │
├───────────┴───────────┴──────────────────┤
│     SQLite WAL · Agent Log Trackers      │
└──────────────────────────────────────────┘
```

**Design:**
- **Agents own truth** — Claude's JSONL logs are authoritative for session identity, usage, and tool calls. Potato reads them, never invents state.
- **PTY-first** — Real terminals. Not rendered transcripts.
- **Pure state** — Elm-style reducer. Side-effect-free rendering.
- **MCP-native** — Coordination through the standard Model Context Protocol, not custom IPC.

## What's Next

- [ ] Per-project state (`.potato/` — roles, context, tasks persist between sessions)
- [ ] Project-scoped agent roster
- [ ] Coordination observatory sidebar
- [ ] `cargo install potato`
- [ ] Homebrew tap

## Development

```bash
cargo test          # 663 tests
cargo build --release
RUST_LOG=debug cargo run
```

~25k lines of Rust · 66 modules · 663 tests

## License

[MIT](LICENSE)

---

<sub>Built with [ratatui](https://github.com/ratatui/ratatui) · [portable-pty](https://github.com/wez/wezterm/tree/main/pty) · [vt100](https://github.com/doy/vt100-rust) · [tui-term](https://github.com/a-kenji/tui-term)</sub>
