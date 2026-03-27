# 🥔 Potato

> Terminal-native AI agent orchestration desktop.

Potato is a keyboard-driven TUI for running local and cloud LLMs with full tool-use support — file editing, shell execution, search, and more — all inside your terminal.

## Getting Started

```bash
cargo run
```

To use a specific model:

```bash
cargo run -- --model llama3.2
```

To use a custom config:

```bash
cargo run -- --config /path/to/config.toml
```

Release build:

```bash
cargo build --release
./target/release/potato
```

## Features (Roadmap)

- 🧠 Local Ollama + cloud LLM support
- 🔧 Built-in tools: shell, file read/write/edit, search, directory listing
- 📋 Tool approval gate — you decide what runs
- 💬 Multi-session history with SQLite persistence
- 🎨 Earth-tone theme with syntax highlighting
- 📊 Live token usage dashboard
- ⚡ Fuzzy slash-command menu

## Project Structure

```
src/
├── main.rs          — Entry point, CLI args, terminal setup
├── app/             — State, messages, update loop, actions
├── ui/              — Layout, theme, panels, widgets, overlays
│   ├── panels/      — Chat, tool output, file preview, sessions, etc.
│   ├── widgets/     — Reusable ratatui components
│   └── overlays/    — Modal dialogs (slash menu, model picker, help)
├── agent/           — Agent loop, state machine, streaming, approvals
├── ollama/          — LLM client trait + local/cloud implementations
├── tools/           — Tool trait, registry, executor, built-ins
├── session/         — SQLite store, message history, export
├── config/          — Schema, defaults, keybinds
└── terminal/        — Event stream, panic hook
```

## Requirements

- Rust 1.85+
- [Ollama](https://ollama.ai) running locally (or configure a cloud endpoint)
