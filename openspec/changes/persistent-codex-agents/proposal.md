## Why

Potato’s value is not “run a CLI once,” it is “keep a roster of active agents you can talk to.” Codex currently breaks that model because Potato treats it like an embedded terminal app, which creates a TUI-in-TUI mess and leaves session continuity, MCP setup, and observability half-working.

## What Changes

- Make Codex orchestration **exec-first**: default Codex work runs through `codex exec --json` and `codex exec resume --json`, not a permanently embedded interactive TUI.
- Define a persistent Codex agent model: each active Codex pane keeps a local Potato identity plus the native Codex thread/session id used for later turns.
- Add Codex turn lifecycle handling so Potato can send another message to the same active Codex agent, track whether it is idle/running/waiting, and reject or queue overlapping sends safely.
- Replace the raw-terminal assumption for Codex panes with a Potato-rendered agent view showing transcript, status, tool activity, and latest result.
- Add Codex MCP setup checks so Potato can detect when Codex does not have the `potato mcp-server` registered and show corrective guidance.
- Keep an explicit interactive/native Codex takeover path optional, but stop making it the default orchestration backend.

## Capabilities

### New Capabilities
- `codex-runtime`: Persistent, messageable Codex agents backed by `codex exec` / `resume`, native thread identity, and Potato-rendered status/transcript views.

### Modified Capabilities
- None.

## Impact

- Affected code: `src/adapters/codex.rs`, `src/main.rs`, `src/pty/`, `src/app/state.rs`, `src/session/store.rs`, `src/session/discovery.rs`, `src/ui/screens/session.rs`, MCP/config wiring, docs.
- Affected behavior: Codex becomes a first-class persistent backend for active agents instead of a fragile embedded TUI lane.
- External dependency: Codex MCP registration must be checked against Codex’s own MCP configuration flow rather than relying on `.mcp.json` alone.
