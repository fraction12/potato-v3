# Tasks — persistent-codex-agents

- [ ] T-1101: Generalize provider-native session identity — replace Claude-specific native session fields/naming with provider-neutral `native_agent_session_id` storage across app state, reducers, persistence, and UI.
- [ ] T-1102: Add Codex exec runner — implement a non-interactive Codex execution path using `codex exec --json` for first turns and wire JSONL events into Potato state updates.
- [ ] T-1103: Add Codex resume path — send subsequent turns to active Codex agents via `codex exec resume <thread_id> --json`.
- [ ] T-1104: Enforce single-flight turns per Codex agent — reject or explicitly queue overlapping sends to the same Codex thread.
- [ ] T-1105: Replace raw Codex PTY pane assumptions — render transcript, status, latest result, and tool activity for exec-backed Codex panes inside Potato.
- [ ] T-1106: Persist native Codex thread identity and session metadata — update SQLite/session store and log attachment flow to use the real Codex thread id once known.
- [ ] T-1107: Add Codex MCP registration check and operator guidance — detect missing `potato mcp-server` registration for Codex and show corrective setup instructions.
- [ ] T-1108: Add tests for first-turn bind, resume, overlapping send handling, persistence, and pane rendering for exec-backed Codex agents.
- [ ] T-1109: Document the new Codex model — update README/CLAUDE/docs to explain exec-backed persistent agents and optional native takeover mode.
- [ ] T-1110: Add optional native Codex takeover mode behind an explicit user action, not the default spawn path.
