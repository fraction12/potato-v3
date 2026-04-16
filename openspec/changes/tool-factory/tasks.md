# Tool Factory — Tasks

## Task 1: Define CustomToolDef and ToolInput structs
**File:** `src/mcp/state.rs`
- [ ] Add `CustomToolDef` struct (name, description, version, command, timeout_secs, inputs, tool_dir)
- [ ] Add `ToolInput` struct (key, input_type, description, required)
- [ ] Add `ToolInputType` enum (String, Number, Boolean)
- [ ] Add `ToolScanError` struct (directory, error message)
- [ ] Add `ToolRunResult` struct (status, exit_code, stdout, stderr, duration_ms)
- [ ] Add `custom_tools: Vec<CustomToolDef>` field to `InterSessionState`
- [ ] Tests: struct creation, default values, serialization

## Task 2: Implement tool.toml parser and directory scanner
**File:** `src/mcp/tools_factory.rs` (new module)
- [ ] `parse_tool_manifest(path: &Path) -> Result<CustomToolDef, ToolScanError>` — parse tool.toml via `toml` crate
- [ ] Validate: name matches folder name, required fields present, name max 64 chars alphanumeric+hyphens, description max 500 chars, timeout 1–300
- [ ] `scan_tools(project_root: &Path) -> (Vec<CustomToolDef>, Vec<ToolScanError>)` — iterate `.potato/tools/` subdirs
- [ ] Skip non-directories, skip folders without tool.toml, collect errors per-folder
- [ ] Tests: valid manifest parsing, missing fields, name mismatch, empty directory, no tools directory (returns empty), timeout clamping

## Task 3: Implement tool execution engine
**File:** `src/mcp/tools_factory.rs`
- [ ] `validate_args(tool: &CustomToolDef, args: &HashMap<String, Value>) -> Result<(), Vec<String>>` — check required, types, no extra keys
- [ ] `run_tool(tool: &CustomToolDef, args: HashMap<String, Value>) -> ToolRunResult` — spawn child process
- [ ] Build env vars: `POTATO_ARG_<UPPER_KEY>=<value>` + `POTATO_TOOL_NAME` + `POTATO_PROJECT_ROOT`
- [ ] Use `tokio::process::Command` with `sh -c`, cwd = tool_dir, timeout via `tokio::time::timeout`
- [ ] Capture stdout + stderr separately
- [ ] Return appropriate error codes: TOOL_NOT_FOUND, VALIDATION_FAILED, TOOL_EXECUTION_FAILED, TOOL_TIMEOUT, TOOL_SPAWN_FAILED
- [ ] Tests: successful execution, non-zero exit, timeout, missing required arg, type mismatch, extra arg rejection

## Task 4: Wire MCP tools — potato_list_tools, potato_run_tool, potato_reload_tools
**File:** `src/mcp/tools.rs`
- [ ] Register three new tools in `handle_tool_call` dispatch
- [ ] `potato_list_tools`: read from `InterSessionState::custom_tools`, return JSON with tools array + count
- [ ] `potato_run_tool`: lookup tool by name, validate args, call `run_tool()`, return structured response
- [ ] `potato_reload_tools`: call `scan_tools()`, update `InterSessionState::custom_tools` AND `AppState::custom_tools`, return tool count + errors
- [ ] Include `_meta` (team roster) in all three responses
- [ ] Tests: list empty, list with tools, run valid tool, run missing tool, run with bad args, reload picks up new tools

## Task 5: Sync MCP state with AppState for UI
**File:** `src/main.rs`, `src/mcp/state.rs`
- [ ] On startup: call `scan_tools()`, populate both `AppState::custom_tools` and `InterSessionState::custom_tools`
- [ ] On `potato_reload_tools`: update both state sources so UI reflects changes on next render tick
- [ ] Bridge the `ToolInfo` (UI) and `CustomToolDef` (MCP) types — either unify or add a conversion
- [ ] Tests: startup scan wires to both states, reload updates both

## Task 6: Add tool.toml schema to CLAUDE.md / agent instructions
**File:** `CLAUDE.md` or `.potato/` agent instructions
- [ ] Document `.potato/tools/` convention
- [ ] Show example tool.toml with all fields
- [ ] Explain argument passing via env vars
- [ ] Explain `potato_reload_tools` call after creating tools
- [ ] One-line system prompt addition: "Custom tools live in .potato/tools/. Use potato_list_tools to discover them."

## Task 7: Integration test — end-to-end tool lifecycle
**File:** `tests/test_tool_factory.rs` (new)
- [ ] Create temp `.potato/tools/echo-test/` with tool.toml + simple echo script
- [ ] Scan → verify tool appears in list
- [ ] Run → verify output matches expected
- [ ] Add second tool → reload → verify both appear
- [ ] Remove tool → reload → verify it's gone
- [ ] Malformed tool.toml → scan reports error, other tools unaffected
