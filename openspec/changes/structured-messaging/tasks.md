# Tasks — structured-messaging

## Summary

Replace freeform string messages in `potato_send_message` with a validated structured schema. The MCP tool enforces the contract — agents that send malformed messages get a clear error and must retry. No UI changes; injection formatting adapts to render structured content cleanly.

## Motivation

Agents currently send raw markdown documents (headers, bold, code blocks, escaped newlines) as message content. This produces unreadable walls of text in the PTY, wastes tokens, and makes messages impossible to parse programmatically. The fix is protocol-level: define a schema, validate on input, reject with actionable errors.

## Schema

```json
{
  "to": "partner",
  "priority": "normal",
  "type": "task | status | question | result",
  "subject": "T-812: Wire up project-scoped agent roster",
  "body": {
    "summary": "ProfileLoader exists but is never called. Need to wire it into AppState and agent picker.",
    "files": ["src/config/profiles.rs", "src/app/state.rs"],
    "steps": [
      "Rename profiles.toml to agents.toml",
      "Feed loaded profiles into AppState.agents",
      "Update agent picker to read from AppState"
    ],
    "context": "Optional extra detail if needed"
  }
}
```

## Validation Rules

- `type` — required, must be one of: `task`, `status`, `question`, `result`
- `subject` — required, max 200 chars, plain text only
- `body.summary` — required, max 500 chars, plain text only
- `body.files` — optional, array of strings (file paths)
- `body.steps` — optional, array of strings, each max 200 chars
- `body.context` — optional, max 1000 chars
- **No markdown anywhere** — reject if any field contains `**`, `###`, or triple backticks
- On rejection: return `CallToolResult::failure` with the exact expected schema and specific errors

## Tasks

- [ ] T-1010: Update `potato_send_message` tool definition — replace `message` string with structured schema (`type`, `subject`, `body` object). Update tool description to explain format and validation. File: `src/mcp/tools.rs`
- [ ] T-1011: Add message validation in `handle_send_message` — validate `type` enum, field lengths, plain-text-only (reject markdown markers). Return clear error with expected schema on failure. File: `src/mcp/tools.rs`
- [ ] T-1012: Serialize validated message as JSON into `InterMessage.content` — keep `InterMessage` struct unchanged, store structured payload as JSON string in existing `content: String` field. File: `src/mcp/tools.rs`, `src/mcp/state.rs`
- [ ] T-1013: Update `format_notification` for structured content — extract `type`, `subject`, summary from JSON payload. Render as: `[Potato: Pane 0 (architect)] [task] T-812: Wire up agent roster | 4 steps, 3 files`. Fall back to raw content for legacy/malformed messages. File: `src/mcp/injection.rs`
- [ ] T-1014: Update existing tests and add validation tests — fix tests that pass freeform `message` string. Add tests for: valid structured message, missing required fields, markdown rejection, field length limits, legacy fallback in injection. Files: `src/mcp/tools.rs`, `src/mcp/injection.rs`

## Files Affected

- `src/mcp/tools.rs` — tool definition + handler + validation (primary)
- `src/mcp/injection.rs` — notification formatting
- `src/mcp/state.rs` — no struct changes, but content semantics change
- `src/mcp/project_store.rs` — no changes needed (stores content as text, JSON fits)
- `src/mcp/protocol.rs` — no changes needed
- `src/mcp/server.rs` — no changes needed
- `src/mcp/bridge.rs` — no changes needed
