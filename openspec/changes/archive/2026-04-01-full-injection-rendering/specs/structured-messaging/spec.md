## MODIFIED Requirements

### Requirement: Full PTY injection rendering (replaces: Compact PTY injection rendering)

`format_notification` SHALL attempt to parse `InterMessage.content` as structured JSON. On success, it SHALL render a multi-line block containing all message fields.

The rendered block SHALL follow this format:

```
[Potato: Pane {id} ({role})] [{type}] {subject}

{body.summary}

Steps:
1. {step_1}
2. {step_2}
...

Files: {file_1}, {file_2}, ...

Context: {body.context}
[/Potato]
```

The header line (`[Potato: ...]`) SHALL always be present. The `body.summary` paragraph SHALL always be present (it is a required field). The `Steps:`, `Files:`, and `Context:` sections SHALL only be rendered if their corresponding fields are present and non-empty. The block SHALL always end with `[/Potato]`.

#### Scenario: Full structured message injection
- **WHEN** a structured message with `type: "task"`, `subject: "T-812: Wire up agent roster"`, `body.summary: "ProfileLoader exists but is never called."`, 4 steps, 3 files, and a context string is injected
- **THEN** the PTY injection SHALL render a multi-line block with header, summary paragraph, numbered steps list, comma-separated files line, context paragraph, and `[/Potato]` closing tag

#### Scenario: Structured message with summary only
- **WHEN** a structured message with `type: "question"`, `subject: "Which DB tool?"`, and `body.summary: "Should we use refinery or sqlx-migrate?"` (no steps, files, or context) is injected
- **THEN** the PTY injection SHALL render the header line, a blank line, the summary, and `[/Potato]` — with no Steps, Files, or Context sections

#### Scenario: Structured message with partial optional fields
- **WHEN** a structured message has steps and files but no context
- **THEN** the PTY injection SHALL render header, summary, Steps section, Files section, and `[/Potato]` — with no Context section

#### Scenario: Legacy content fallback (unchanged)
- **WHEN** `format_notification` receives content that does not parse as structured JSON
- **THEN** it SHALL fall back to the current behavior: sanitize control characters and render as a flat one-liner

#### Scenario: Malformed JSON fallback (unchanged)
- **WHEN** `format_notification` receives valid JSON that lacks `type` or `subject` fields
- **THEN** it SHALL fall back to the legacy sanitized one-liner rendering
