# structured-messaging Specification

## Purpose
TBD - created by archiving change structured-messaging. Update Purpose after archive.
## Requirements
### Requirement: Structured message input schema

The `potato_send_message` MCP tool SHALL accept a structured object instead of a freeform `message` string. The input schema SHALL be:

- `to` (string, optional): Target pane — `"partner"` or numeric pane ID string. Defaults to partner resolution.
- `priority` (string, optional): `"normal"` or `"urgent"`. Defaults to `"normal"`.
- `type` (string, required): One of `"task"`, `"status"`, `"question"`, `"result"`.
- `subject` (string, required): Plain text, max 200 characters.
- `body` (object, required):
  - `summary` (string, required): Plain text, max 500 characters.
  - `files` (array of strings, optional): File paths relevant to the message.
  - `steps` (array of strings, optional): Each max 200 characters.
  - `context` (string, optional): Plain text, max 1000 characters.

#### Scenario: Valid structured message is accepted
- **WHEN** an agent sends a message with `type: "task"`, `subject: "Wire up profiles"`, and `body: { summary: "ProfileLoader needs wiring" }`
- **THEN** the tool SHALL return success with delivery confirmation

#### Scenario: Legacy freeform message field is rejected
- **WHEN** an agent sends a message using only the old `message: string` field without `type`, `subject`, and `body`
- **THEN** the tool SHALL return a failure with the expected schema and list of missing fields

### Requirement: Message type validation

The `type` field SHALL be validated against the enum `["task", "status", "question", "result"]`. Any other value SHALL be rejected.

#### Scenario: Invalid type value
- **WHEN** an agent sends a message with `type: "announcement"`
- **THEN** the tool SHALL return a failure stating the valid type values

### Requirement: Field length enforcement

The tool SHALL enforce maximum character lengths: `subject` max 200, `body.summary` max 500, each `body.steps` item max 200, `body.context` max 1000. Messages exceeding any limit SHALL be rejected.

#### Scenario: Subject exceeds length limit
- **WHEN** an agent sends a message with a `subject` longer than 200 characters
- **THEN** the tool SHALL return a failure identifying the field and its limit

#### Scenario: Steps item exceeds length limit
- **WHEN** an agent sends a message with a `body.steps` entry longer than 200 characters
- **THEN** the tool SHALL return a failure identifying which steps item exceeded the limit

### Requirement: Plain-text-only enforcement

All string fields (`subject`, `body.summary`, `body.steps` items, `body.context`) SHALL contain plain text only. The tool SHALL reject messages where any field contains `**`, `###`, or triple backticks (`` ``` ``).

#### Scenario: Markdown bold in summary
- **WHEN** an agent sends a message where `body.summary` contains `**important**`
- **THEN** the tool SHALL return a failure stating that markdown is not allowed and identifying the field

#### Scenario: Code fence in context
- **WHEN** an agent sends a message where `body.context` contains triple backticks
- **THEN** the tool SHALL return a failure stating that markdown is not allowed and identifying the field

### Requirement: Actionable error responses

When validation fails, the tool SHALL return a `CallToolResult::failure` containing: the specific validation errors, and the full expected schema so the agent can self-correct.

#### Scenario: Multiple validation errors
- **WHEN** an agent sends a message with missing `type`, a `subject` over 200 chars, and markdown in `body.summary`
- **THEN** the tool SHALL return a single failure listing all errors, not just the first one

### Requirement: Structured content storage

Validated messages SHALL be serialized as a JSON object and stored in `InterMessage.content` as a string. The JSON object SHALL contain `type`, `subject`, and `body` fields.

#### Scenario: Content field contains valid JSON
- **WHEN** a structured message is successfully sent
- **THEN** `InterMessage.content` SHALL contain a JSON string that deserializes to an object with `type`, `subject`, and `body` keys

### Requirement: Compact PTY injection rendering

`format_notification` SHALL attempt to parse `InterMessage.content` as structured JSON. On success, it SHALL render a compact one-liner: `[Potato: Pane {id} ({role})] [{type}] {subject} | {summary_preview}`. File count and step count MAY be appended if present.

#### Scenario: Structured message injection
- **WHEN** an urgent structured message with `type: "task"`, `subject: "T-812: Wire up agent roster"`, 4 steps, and 3 files is injected
- **THEN** the PTY injection SHALL render as `[Potato: Pane 0 (architect)] [task] T-812: Wire up agent roster | 4 steps, 3 files`

#### Scenario: Legacy content fallback
- **WHEN** `format_notification` receives content that does not parse as structured JSON
- **THEN** it SHALL fall back to the current behavior: sanitize control characters and render as a flat one-liner

