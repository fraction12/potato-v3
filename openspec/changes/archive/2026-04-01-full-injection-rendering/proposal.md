# Proposal: Full Injection Rendering

## Problem

PTY injection currently renders structured messages as a compact one-liner:

```
[Potato: Pane 0 (architect)] [task] T-812: Wire up agent roster | 4 steps, 3 files
```

This strips **all actionable content**: the summary, step descriptions, file paths, and context. The agent sees metadata counts ("4 steps, 3 files") but never the actual instructions.

Since PTY injection is the **only push delivery mechanism** — there is no polling loop on `potato_get_messages` — the injected text is the only thing the agent will reliably process. The full message body sits unread in the inbox indefinitely.

## Root Cause

`format_notification()` in `src/mcp/injection.rs` was designed for a "notification + pull" model. It assumed agents would see the one-liner, then call `potato_get_messages` to retrieve the full content. In practice, agents have no reason to call `get_messages` — they treat the injected PTY text as the complete user message and act on it directly.

## Proposed Solution

Replace the compact one-liner rendering with a full multi-line block that includes all structured message fields. The injection already goes in as a user-turn message, so the agent will process the entire block.

### New format

```
[Potato: Pane 0 (architect)] [task] T-812: Wire up agent roster

ProfileLoader exists but is never called from the startup flow.

Steps:
1. Rename profiles.toml to agents.toml
2. Feed loaded profiles into AppState::new()
3. Update the agent picker overlay to read from state
4. Add integration test for role loading

Files: src/config/profiles.rs, src/app/state.rs, src/ui/overlays/agent_picker.rs

Context: This blocks the agent picker overlay — without loaded profiles, the picker renders an empty list.
[/Potato]
```

### Key design decisions

1. **Keep the header line** — `[Potato: Pane {id} ({role})] [{type}] {subject}` remains as the first line for easy visual identification.
2. **Render `body.summary` as the main paragraph** — always present (required field).
3. **Render `body.steps` as a numbered list** — only if present.
4. **Render `body.files` as a comma-separated line** — only if present.
5. **Render `body.context` as a labeled paragraph** — only if present.
6. **Close with `[/Potato]`** — clear delimiter so the agent knows where the message ends.
7. **Legacy fallback unchanged** — non-JSON content still gets the single-line sanitized treatment.
8. **No newline sanitization for structured messages** — the current control-char stripping only applies to the legacy path. Structured messages use actual newlines for readability.

### What this does NOT change

- `send_message()` storage — `InterMessage.content` remains the same JSON string.
- `get_messages()` return format — unchanged.
- `InjectRequest` struct — unchanged.
- `inject_into_pane()` — unchanged (writes whatever text it receives).
- `PendingEnter` / `ENTER_DELAY_TICKS` — unchanged.
- Approval-pending guard — unchanged.

## Impact

- **`src/mcp/injection.rs`**: `format_notification()` rewritten for structured path; legacy fallback preserved.
- **Tests**: Existing structured-message tests updated to expect multi-line output. New tests for each optional field combination.
- **Spec delta**: `structured-messaging` spec requirement "Compact PTY injection rendering" updated to "Full PTY injection rendering."

## Risks

- **Long messages in PTY**: A message with many steps/files could produce a large block. Mitigated by existing field length limits (subject 200, summary 500, context 1000, steps 200 each).
- **Claude Code Ink rendering**: Multi-line paste into PTY. The existing `ENTER_DELAY_TICKS` mechanism already handles the text→Enter separation. No bracketed paste needed (per commit 112096f).
