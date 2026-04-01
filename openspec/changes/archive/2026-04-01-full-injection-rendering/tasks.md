# Tasks: Full Injection Rendering

## Tasks

### Task 1: Extract `format_structured_notification` helper
- [ ] Create `format_structured_notification(prefix, msg_type, subject, body) -> String` in `src/mcp/injection.rs`
- [ ] Render header line: `{prefix} [{msg_type}] {subject}`
- [ ] Render blank separator + `body.summary` (always present)
- [ ] Render `Steps:` numbered list if `body.steps` present and non-empty
- [ ] Render `Files:` comma-separated line if `body.files` present and non-empty
- [ ] Render `Context:` paragraph if `body.context` present and non-empty
- [ ] Close with `[/Potato]`
- [ ] Return joined with `\n`

### Task 2: Update `format_notification` structured branch
- [ ] Replace the existing structured JSON branch in `format_notification()` to call `format_structured_notification()` instead of building the compact one-liner
- [ ] Preserve the prefix construction logic (role suffix)
- [ ] Preserve the legacy fallback path (non-JSON and JSON-without-type/subject)

### Task 3: Update existing tests
- [ ] Update `format_notification_structured_with_steps_and_files` — expect multi-line block instead of one-liner
- [ ] Update `format_notification_structured_no_steps_or_files` — expect header + summary + closing tag
- [ ] Update `format_notification_structured_steps_only` — expect Steps section, no Files section
- [ ] Update `format_notification_structured_files_only` — expect Files section, no Steps section

### Task 4: Add new tests
- [ ] `format_notification_structured_full` — all fields present (summary, steps, files, context)
- [ ] `format_notification_structured_summary_only` — only required fields
- [ ] `format_notification_structured_has_closing_tag` — all structured messages end with `[/Potato]`
- [ ] `format_notification_structured_context_only` — summary + context, no steps/files
- [ ] `format_notification_legacy_unchanged` — plain text still single-line (regression guard)
- [ ] `format_notification_malformed_json_unchanged` — JSON without type/subject falls back (regression guard)
