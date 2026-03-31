## Why

Agents currently send raw markdown documents as MCP message content — headers, bold, code blocks, escaped newlines. This produces unreadable walls of text when injected into the PTY, wastes tokens on formatting that gets stripped anyway, and makes messages impossible to parse programmatically. A protocol-level schema enforces structure at the source, giving agents clear errors when they send malformed messages.

## What Changes

- **BREAKING**: `potato_send_message` replaces freeform `message: string` with a structured object (`type`, `subject`, `body` with `summary`/`files`/`steps`/`context`)
- Add server-side validation: required fields, length limits, plain-text-only enforcement (reject markdown markers)
- Return actionable error messages with the expected schema on validation failure
- Store validated structured payload as JSON string in existing `InterMessage.content` field (no struct changes)
- Update PTY injection formatting to extract and render structured fields as a compact one-liner
- Legacy/malformed messages in `format_notification` fall back to current raw-content rendering

## Capabilities

### New Capabilities
- `structured-messaging`: Validated structured message schema for inter-agent MCP communication, including type classification, field constraints, plain-text enforcement, and compact PTY injection rendering

### Modified Capabilities
<!-- None — no existing specs are affected -->

## Impact

- `src/mcp/tools.rs` — tool definition schema change, new validation logic in handler (**BREAKING** for any agent calling `potato_send_message` with the old `message` field)
- `src/mcp/injection.rs` — `format_notification` updated to parse structured JSON and render compact format
- `src/mcp/state.rs` — no struct changes; `InterMessage.content` now stores JSON strings instead of freeform text
- Existing tests that pass freeform `message` strings must be updated
- No UI changes, no database schema changes, no new dependencies
