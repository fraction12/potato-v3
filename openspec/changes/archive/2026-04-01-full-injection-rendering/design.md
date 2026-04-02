# Design: Full Injection Rendering

## Architecture

No new modules, structs, or channels. This change is entirely within `format_notification()` in `src/mcp/injection.rs`.

## Detailed Design

### `format_notification(from_pane, from_role, content) -> String`

**Structured path** (content parses as JSON with `type` + `subject`):

```rust
fn format_structured_notification(
    prefix: &str,       // "[Potato: Pane 0 (architect)]"
    msg_type: &str,     // "task"
    subject: &str,      // "T-812: Wire up agent roster"
    body: &Value,       // the body object
) -> String {
    let mut lines = Vec::new();

    // Line 1: header
    lines.push(format!("{prefix} [{msg_type}] {subject}"));

    // Line 2: blank separator
    lines.push(String::new());

    // Line 3+: summary (always present — required field)
    if let Some(summary) = body.get("summary").and_then(Value::as_str) {
        lines.push(summary.to_string());
    }

    // Steps: numbered list
    if let Some(steps) = body.get("steps").and_then(Value::as_array) {
        if !steps.is_empty() {
            lines.push(String::new());
            lines.push("Steps:".to_string());
            for (i, step) in steps.iter().enumerate() {
                if let Some(s) = step.as_str() {
                    lines.push(format!("{}. {s}", i + 1));
                }
            }
        }
    }

    // Files: single line, comma-separated
    if let Some(files) = body.get("files").and_then(Value::as_array) {
        let file_strs: Vec<&str> = files.iter().filter_map(Value::as_str).collect();
        if !file_strs.is_empty() {
            lines.push(String::new());
            lines.push(format!("Files: {}", file_strs.join(", ")));
        }
    }

    // Context: labeled paragraph
    if let Some(ctx) = body.get("context").and_then(Value::as_str) {
        if !ctx.is_empty() {
            lines.push(String::new());
            lines.push(format!("Context: {ctx}"));
        }
    }

    // Closing delimiter
    lines.push("[/Potato]".to_string());

    lines.join("\n")
}
```

**Legacy path** (unchanged): sanitize control chars, flatten to single line.

### Control character handling

- **Structured path**: No sanitization. The content has already been validated by `validate_structured_message()` (plain text only, no markdown). Newlines within individual fields are preserved if present (though current validation doesn't allow them — future-proofing).
- **Legacy path**: Existing sanitization unchanged — replace control chars with spaces, collapse double spaces.

### PTY write safety

No changes to `inject_into_pane()`. The function writes the full text block, then `drain_inject_requests()` sends `\r` after `ENTER_DELAY_TICKS`. Multi-line text works because it's written as a single `write_input()` call — the PTY receives it as one paste, and the deferred `\r` submits it.

## Files Modified

| File | Change |
|------|--------|
| `src/mcp/injection.rs` | Rewrite structured branch of `format_notification()` to render full multi-line block. Extract `format_structured_notification()` helper. Update/add tests. |

## Files NOT Modified

| File | Reason |
|------|--------|
| `src/mcp/state.rs` | `send_message()` passes `content` unchanged — no change needed |
| `src/mcp/tools.rs` | `handle_send_message()` serialization unchanged |
| `src/main.rs` | `drain_inject_requests()` calls `format_notification()` — no change needed |

## Test Plan

1. **`format_notification_structured_full`** — message with all fields (type, subject, summary, steps, files, context) → verify multi-line output with all sections.
2. **`format_notification_structured_summary_only`** — message with only required fields → verify header + summary + closing tag, no Steps/Files/Context sections.
3. **`format_notification_structured_no_steps`** — message with summary + files + context but no steps → verify Steps section absent.
4. **`format_notification_structured_no_files`** — message with summary + steps + context but no files → verify Files section absent.
5. **`format_notification_structured_no_context`** — message with summary + steps + files but no context → verify Context section absent.
6. **`format_notification_structured_closing_tag`** — all structured messages end with `[/Potato]`.
7. **`format_notification_legacy_unchanged`** — plain text still renders as single-line with control char sanitization.
8. **`format_notification_malformed_json_unchanged`** — JSON without type/subject still falls back to legacy.
