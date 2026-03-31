## Context

Potato's `potato_send_message` MCP tool currently accepts a freeform `message: string`. Agents (especially Claude Code) tend to fill this with verbose markdown — headers, bold, code fences, escaped newlines — which gets control-char-stripped into an unreadable single line when injected into the partner's PTY. The `InterMessage` struct stores `content: String` and is persisted to SQLite via `project_store.rs` as plain text.

The injection path is: `handle_send_message` → `state.send_message()` → inbox queue + optional `project_store.log_message()`. For urgent messages, `format_notification` renders a one-liner injected directly into the PTY.

## Goals / Non-Goals

**Goals:**
- Define a structured message schema that agents must follow
- Validate all fields server-side with clear, actionable error messages
- Keep `InterMessage` struct unchanged — store structured payload as JSON in `content: String`
- Update PTY injection to render structured content as a compact, readable one-liner
- Maintain backward compatibility in `format_notification` for any legacy messages already in the inbox

**Non-Goals:**
- No changes to `InterMessage` struct, SQLite schema, or `project_store.rs`
- No UI changes (sidebar panels, overlays)
- No changes to `potato_get_messages` output format (it returns `InterMessage` as-is; consumers parse the JSON `content`)
- No versioning or negotiation — this is a clean break, old format is simply rejected
- No changes to `protocol.rs`, `server.rs`, or `bridge.rs`

## Decisions

### 1. Validate in the handler, not in a shared layer

**Choice:** All validation happens in `handle_send_message` in `tools.rs`.

**Rationale:** Only one tool sends messages. Adding a validation layer or middleware would be premature abstraction. The handler already does argument parsing and error returns — validation is a natural extension.

**Alternative considered:** A `StructuredMessage` type with `TryFrom<Value>` — cleaner separation but adds a type that only one call site uses. Not worth it at this scale.

### 2. Store as JSON string in existing `content: String`

**Choice:** `serde_json::to_string()` the validated fields into `InterMessage.content`. No struct changes.

**Rationale:** `InterMessage` is serialized to SQLite as text and returned via MCP as JSON. Storing structured data as a JSON string inside the existing field avoids migration, schema changes, and breaking `potato_get_messages` consumers. The consumer can `serde_json::from_str` if it needs fields.

**Alternative considered:** Adding typed fields to `InterMessage` — cleaner but requires SQLite migration, changes to `project_store.rs`, and updates to every serialization point. Disproportionate effort for the benefit.

### 3. Reject markdown markers, don't strip them

**Choice:** If any field contains `**`, `###`, or triple backticks, reject the entire message with an error explaining the constraint.

**Rationale:** Stripping silently would let agents think their formatting worked. Rejecting forces agents to send clean content from the start, which is the actual goal. Agents retry with plain text — they learn the contract.

### 4. Legacy fallback in `format_notification`

**Choice:** `format_notification` tries to parse `content` as the structured JSON schema. If parsing fails (old messages, malformed content), it falls back to the current behavior (sanitize + flatten to one line).

**Rationale:** Messages already in the inbox or backing store from before this change still need to render. A try-parse-then-fallback approach handles this without migration.

## Risks / Trade-offs

- **[Breaking change for agents]** Agents calling `potato_send_message` with the old `message` field will get an error. → **Mitigation:** Error message includes the full expected schema so agents can self-correct on retry.
- **[JSON-in-string is not ideal]** Nested JSON string in `content` is awkward to query in SQLite. → **Mitigation:** Accepted trade-off; avoids migration. Can promote to typed fields later if needed.
- **[Markdown detection is heuristic]** Checking for `**`, `###`, triple backticks covers common cases but not all markdown. → **Mitigation:** These are the markers agents actually use. Can expand the blocklist later if needed.
- **[No schema versioning]** If the schema needs to change again, it's another breaking change. → **Mitigation:** Acceptable for an internal protocol between Potato and its spawned agents. Not a public API.
