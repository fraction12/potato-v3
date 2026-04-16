# Persistent Codex Agents — Design

## Summary

Potato should treat Codex as a persistent thread-backed agent runtime, not as a permanently embedded terminal UI. The default Codex lane becomes `codex exec --json` for first turns and `codex exec resume <thread_id> --json` for subsequent turns. Potato owns the visible pane, transcript, status, and routing; Codex owns thread continuity and tool execution.

## Goals

- Preserve Potato’s core UX: a roster of active agents that can be messaged repeatedly.
- Make Codex reliable inside Potato without nesting one TUI inside another.
- Persist the native Codex thread id separately from Potato’s local pane/session id.
- Surface status, transcript, and tool activity in Potato even when Codex runs non-interactively.
- Keep MCP-based coordination available to Codex agents.

## Non-Goals

- Rewriting Claude’s existing PTY-first path.
- Making interactive Codex takeover the default path.
- Solving every provider with one abstraction pass before Codex works.

## Architecture

### 1. Active Codex agent model

Each Codex pane should represent a logical agent with two identities:

- `pane_session_id`: Potato-local identity used by panes, routing, and UI selection.
- `native_agent_session_id`: the real Codex thread id returned by `thread.started`.

Recommended state additions:

- `transport_mode: ExecJson | InteractiveTakeover`
- `native_agent_session_id: Option<String>`
- `turn_state: Idle | Running | WaitingForInput | Failed`
- `last_result_summary: Option<String>`
- `last_tool_activity: Vec<...>` or equivalent derived event timeline
- `pending_user_messages: VecDeque<...>` if queued sends are supported

Important cleanup: the current `claude_session_id` field should be generalized to something provider-neutral like `native_agent_session_id`.

### 2. Turn execution model

#### First turn

When a new Codex agent receives its first user message, Potato should launch:

- `codex exec --json ...`

The prompt is passed via stdin or argument. Potato parses JSONL events and waits for:

- `thread.started` → bind `native_agent_session_id`
- `item.started` / `item.completed` → tool activity
- `item.completed` with `agent_message` → agent response
- `turn.completed` → usage + terminal state transition back to idle

#### Subsequent turns

When the same agent receives another user message and `native_agent_session_id` is known, Potato should launch:

- `codex exec resume <thread_id> --json ...`

This preserves continuity while still keeping Potato in control of rendering and orchestration.

### 3. Concurrency rules

Codex exec runs should be single-flight per agent.

Rules:
- If a Codex agent is already running, Potato shall not start a second overlapping turn for that same agent.
- The UI may either reject a second send with a clear message or enqueue it explicitly.
- Different Codex agents may run concurrently.

This avoids transcript corruption and weird interleaving against one native Codex thread.

### 4. Pane rendering model

Exec-backed Codex panes should stop pretending to be raw terminal views.

Instead, Potato should render:
- agent name / role
- current state: idle, running, waiting, failed
- most recent user message
- most recent Codex response
- tool timeline / latest tool executions
- native thread/session id
- MCP status / warnings

This preserves the “talk to active agents” experience without requiring the user to interact with Codex’s own TUI.

### 5. MCP integration

Potato currently relies on `.mcp.json`, which is useful for Claude-style discovery but not sufficient for Codex.

Codex integration needs an explicit MCP check:
- inspect whether a `potato` server is registered for Codex
- if missing, surface actionable guidance
- optionally provide an install/setup command path later

The minimum viable behavior is detection + guidance, not silent failure.

### 6. Persistence model

SQLite/session storage should preserve:
- adapter = `codex`
- Potato pane/local session id
- native Codex thread id
- title / last summary
- usage totals
- timestamps / turn count

Session discovery should not assume Claude’s directory layout. Codex log lookup should attach only after the native thread id is known.

## Implementation Plan

### Phase A — Provider-neutral session identity
- Rename `claude_session_id` to `native_agent_session_id` (or equivalent)
- Update reducers, store, UI labels, and resume logic to stop assuming Claude-only identity

### Phase B — Exec-backed Codex transport
- Add a Codex exec runner that launches `codex exec --json`
- Add resume path using `codex exec resume <thread_id> --json`
- Parse JSONL events into existing `AgentEvent`s or a thin Codex-specific event layer
- Mark per-agent run state transitions cleanly

### Phase C — Codex pane UX
- Replace raw PTY viewport assumptions for exec-backed Codex panes
- Render transcript/status/tool activity from structured events
- Show native thread id and run-state badges

### Phase D — MCP registration checks
- Add startup/spawn-time validation for Codex MCP registration
- Surface a clear warning when `potato mcp-server` is unavailable to Codex
- Document the expected Codex MCP setup path

### Phase E — Optional interactive takeover
- Add an explicit “open native Codex” action for manual intervention/debugging
- Keep it separate from the default exec-backed orchestration path

## Risks

- Mixed-mode complexity if interactive and exec-backed Codex share too much state.
- Event/render gaps if Potato still assumes raw PTY output everywhere.
- Resume correctness if local and native ids stay conflated.
- User confusion if MCP setup failures are silent.

## Open Questions

- Should queued sends be supported immediately, or should running agents reject new turns for v1?
- Should interactive takeover reuse the same native thread or fork a separate session?
- Do we want a generic “structured agent pane” abstraction now, or after Codex works end-to-end?
