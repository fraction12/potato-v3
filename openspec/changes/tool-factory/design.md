# Tool Factory — Design

## Architecture Overview

```
.potato/tools/
├── fly-status/
│   ├── tool.toml          # Manifest: name, description, command, schema
│   └── check.sh           # Executable script
├── lint-check/
│   ├── tool.toml
│   └── run.py
└── db-query/
    ├── tool.toml
    └── query.sh
```

Potato scans `.potato/tools/` at startup and on `potato_reload_tools` calls. Each valid subfolder becomes a tool available via MCP. The scan result is stored in `AppState::custom_tools` (already landed) and in a parallel `Vec<CustomTool>` in the MCP server state for dispatch.

## tool.toml Schema

```toml
[tool]
name = "fly-status"                    # Required. Unique identifier, must match folder name.
description = "Check Fly.io app status and recent deployments"  # Required. Shown to agents.
version = "0.1.0"                      # Optional. Semver. Informational only in V3.
command = "bash check.sh"              # Required. Executed relative to tool directory.
timeout = 30                           # Optional. Seconds. Default: 30. Max: 300.

[tool.input]                           # Optional. If omitted, tool takes no arguments.
# Each key = argument name. Value = { type, description, required }
app_name = { type = "string", description = "Fly app name", required = true }
region = { type = "string", description = "Filter by region", required = false }
```

### Field Rules

| Field | Type | Required | Constraints |
|-------|------|----------|-------------|
| `name` | string | yes | Must match directory name. Alphanumeric + hyphens. Max 64 chars. |
| `description` | string | yes | Max 500 chars. Plain text only. |
| `version` | string | no | Semver format if provided. |
| `command` | string | yes | Shell command. Executed with `sh -c` in the tool's directory as cwd. |
| `timeout` | integer | no | 1–300 seconds. Default 30. |
| `input.<key>.type` | string | yes (per input) | One of: `"string"`, `"number"`, `"boolean"`. |
| `input.<key>.description` | string | no | Shown to agents for context. |
| `input.<key>.required` | boolean | no | Default false. |

### Argument Passing

Arguments are passed as environment variables to the command. Each input key becomes `POTATO_ARG_<UPPER_KEY>`. Example:

```bash
# tool.toml: app_name = { type = "string", required = true }
# Agent calls: potato_run_tool(name="fly-status", args={"app_name": "my-app"})
# Potato executes: POTATO_ARG_APP_NAME="my-app" sh -c "bash check.sh"
```

This avoids shell injection risks from positional arguments and works across all script languages.

## MCP Tools

### potato_list_tools

**Purpose:** Return all discovered custom tools with their descriptions and input schemas.

**Input:** None.

**Output:**
```json
{
  "tools": [
    {
      "name": "fly-status",
      "description": "Check Fly.io app status and recent deployments",
      "version": "0.1.0",
      "inputs": {
        "app_name": { "type": "string", "description": "Fly app name", "required": true },
        "region": { "type": "string", "description": "Filter by region", "required": false }
      }
    }
  ],
  "count": 1,
  "_meta": { ... }
}
```

### potato_run_tool

**Purpose:** Execute a custom tool by name with provided arguments.

**Input:**
```json
{
  "name": "fly-status",
  "args": {
    "app_name": "my-app"
  }
}
```

**Validation (all errors collected, not fail-fast):**
1. Tool `name` must exist in discovered tools.
2. All `required` inputs must be present in `args`.
3. Input types must match declared schema (`string`, `number`, `boolean`).
4. No extra keys beyond declared inputs (strict mode).

**Execution:**
1. Set working directory to tool's subfolder.
2. Set environment variables: `POTATO_ARG_<KEY>=<value>` for each arg. Also set `POTATO_TOOL_NAME`, `POTATO_PROJECT_ROOT`.
3. Spawn child process via `sh -c "<command>"` with timeout.
4. Capture stdout and stderr separately.
5. Return structured response.

**Output (success):**
```json
{
  "status": "success",
  "exit_code": 0,
  "stdout": "...",
  "stderr": "...",
  "duration_ms": 1234,
  "_meta": { ... }
}
```

**Output (failure):**
```json
{
  "status": "error",
  "error_code": "TOOL_EXECUTION_FAILED",
  "exit_code": 1,
  "stdout": "...",
  "stderr": "Permission denied: flyctl not found",
  "duration_ms": 456,
  "_meta": { ... }
}
```

**Error Codes:**
| Code | Condition |
|------|-----------|
| `TOOL_NOT_FOUND` | No tool with given name |
| `VALIDATION_FAILED` | Missing required args, type mismatch, extra keys |
| `TOOL_EXECUTION_FAILED` | Non-zero exit code |
| `TOOL_TIMEOUT` | Exceeded configured timeout |
| `TOOL_SPAWN_FAILED` | Could not start child process |

### potato_reload_tools

**Purpose:** Rescan `.potato/tools/` directory and update the available tool list. Called by agents after creating or modifying tools.

**Input:** None.

**Output:**
```json
{
  "status": "success",
  "tools_found": 3,
  "tools": ["fly-status", "lint-check", "db-query"],
  "errors": [
    { "directory": "broken-tool", "error": "Missing required field: command" }
  ],
  "_meta": { ... }
}
```

Reports scan errors per-tool without failing the entire reload. Valid tools are loaded; invalid ones are skipped with diagnostics.

## State Management

### MCP Server State (src/mcp/state.rs)

Add to `InterSessionState`:
```rust
pub custom_tools: Vec<CustomToolDef>,
```

Where:
```rust
pub struct CustomToolDef {
    pub name: String,
    pub description: String,
    pub version: Option<String>,
    pub command: String,
    pub timeout_secs: u32,
    pub inputs: Vec<ToolInput>,
    pub tool_dir: PathBuf,
}

pub struct ToolInput {
    pub key: String,
    pub input_type: ToolInputType,  // String, Number, Boolean
    pub description: Option<String>,
    pub required: bool,
}
```

### Scan Function

`scan_tools(project_root: &Path) -> (Vec<CustomToolDef>, Vec<ToolScanError>)`

1. Read `{project_root}/.potato/tools/` directory.
2. For each subfolder, attempt to parse `tool.toml`.
3. Validate: name matches folder name, required fields present, types valid.
4. Return (valid_tools, errors) tuple.

Called at startup and on `potato_reload_tools`.

### Execution Function

`run_tool(tool: &CustomToolDef, args: HashMap<String, Value>) -> ToolRunResult`

1. Validate args against `tool.inputs`.
2. Build env vars: `POTATO_ARG_<KEY>` for each arg.
3. Spawn `sh -c "<tool.command>"` with cwd = `tool.tool_dir`, timeout = `tool.timeout_secs`.
4. Capture stdout/stderr via `tokio::process::Command`.
5. Return `ToolRunResult { status, exit_code, stdout, stderr, duration_ms }`.

## Integration with Existing UI

The Tools panel in the session left rail (already landed) reads from `AppState::custom_tools`. The MCP server state and AppState are synced: `potato_reload_tools` updates both. The UI reflects changes on next render tick.

## Security Considerations

- Tools execute with the same permissions as the Potato process. No sandboxing in V3.
- Argument passing via env vars avoids shell injection.
- Timeout enforcement prevents runaway processes.
- Tool manifests are user-created, not agent-created by default (user-directed policy is social, not enforced in code).
