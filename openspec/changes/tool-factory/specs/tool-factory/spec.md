# tool-factory Specification

## ADDED Requirements

### Requirement: Tool manifest discovery

Potato SHALL scan the `.potato/tools/` directory at startup and on `potato_reload_tools` invocation. Each subfolder containing a valid `tool.toml` SHALL be registered as an available custom tool.

#### Scenario: Valid tool discovered at startup
- **WHEN** Potato starts with `.potato/tools/fly-status/tool.toml` containing name, description, and command fields
- **THEN** the tool SHALL appear in `potato_list_tools` output with its declared name, description, and input schema

#### Scenario: Directory without tool.toml is skipped
- **WHEN** `.potato/tools/scratch/` exists without a `tool.toml`
- **THEN** the directory SHALL be silently skipped and not appear in the tool list

#### Scenario: Malformed tool.toml reports error without blocking others
- **WHEN** `.potato/tools/broken/tool.toml` has a missing required field AND `.potato/tools/valid/tool.toml` is correct
- **THEN** `valid` SHALL be registered AND `broken` SHALL be reported in the errors array of `potato_reload_tools`

### Requirement: Tool manifest schema validation

The `tool.toml` manifest SHALL require `name`, `description`, and `command` fields. The `name` field SHALL match the containing directory name (case-sensitive). Names SHALL be alphanumeric plus hyphens, max 64 characters. Descriptions SHALL be max 500 characters plain text.

#### Scenario: Name mismatch with directory
- **WHEN** a tool.toml in `.potato/tools/my-tool/` declares `name = "other-tool"`
- **THEN** the tool SHALL be rejected with an error identifying the name mismatch

#### Scenario: Name exceeds length limit
- **WHEN** a tool.toml declares a `name` longer than 64 characters
- **THEN** the tool SHALL be rejected with an error identifying the constraint

### Requirement: Tool argument passing via environment variables

Arguments declared in `[tool.input]` SHALL be passed to the tool command as environment variables named `POTATO_ARG_<UPPER_KEY>`. Additionally, `POTATO_TOOL_NAME` and `POTATO_PROJECT_ROOT` SHALL always be set.

#### Scenario: String argument passed as env var
- **WHEN** an agent calls `potato_run_tool` with `name="fly-status"` and `args={"app_name": "my-app"}`
- **THEN** the tool script SHALL receive `POTATO_ARG_APP_NAME=my-app` in its environment

#### Scenario: Boolean argument passed as env var
- **WHEN** an agent calls `potato_run_tool` with `args={"verbose": true}` for a tool declaring `verbose = { type = "boolean" }`
- **THEN** the tool script SHALL receive `POTATO_ARG_VERBOSE=true`

### Requirement: Tool input validation

`potato_run_tool` SHALL validate all provided arguments against the tool's declared input schema before execution. All errors SHALL be collected (not fail-fast). Required inputs must be present. Types must match. Extra keys beyond declared inputs SHALL be rejected.

#### Scenario: Missing required argument
- **WHEN** an agent calls `potato_run_tool` without a required input
- **THEN** the tool SHALL NOT execute and SHALL return `VALIDATION_FAILED` with the missing field name

#### Scenario: Type mismatch
- **WHEN** an agent passes a string for a `number` type input
- **THEN** the tool SHALL NOT execute and SHALL return `VALIDATION_FAILED` identifying the field and expected type

#### Scenario: Extra undeclared argument
- **WHEN** an agent passes an argument not declared in the tool's input schema
- **THEN** the tool SHALL NOT execute and SHALL return `VALIDATION_FAILED` listing the unexpected key

### Requirement: Tool execution with timeout

`potato_run_tool` SHALL execute the tool's command via `sh -c` with the tool's directory as working directory. Execution SHALL be terminated if it exceeds the configured `timeout` (default 30 seconds, max 300 seconds).

#### Scenario: Successful execution
- **WHEN** a tool script exits with code 0 and produces stdout output
- **THEN** `potato_run_tool` SHALL return `status: "success"`, the exit code, captured stdout and stderr, and execution duration in milliseconds

#### Scenario: Non-zero exit code
- **WHEN** a tool script exits with a non-zero code
- **THEN** `potato_run_tool` SHALL return `status: "error"`, error code `TOOL_EXECUTION_FAILED`, the exit code, and captured stdout and stderr

#### Scenario: Execution exceeds timeout
- **WHEN** a tool script runs longer than its configured timeout
- **THEN** the process SHALL be killed and `potato_run_tool` SHALL return error code `TOOL_TIMEOUT`

### Requirement: Tool listing via MCP

`potato_list_tools` SHALL return all discovered custom tools with their name, description, version (if set), and input schema. The response SHALL include a `count` field and the standard `_meta` object.

#### Scenario: No custom tools
- **WHEN** `.potato/tools/` is empty or does not exist
- **THEN** `potato_list_tools` SHALL return an empty `tools` array with `count: 0`

#### Scenario: Multiple tools listed
- **WHEN** three valid tools exist in `.potato/tools/`
- **THEN** `potato_list_tools` SHALL return all three with complete metadata

### Requirement: Tool reload via MCP

`potato_reload_tools` SHALL rescan the `.potato/tools/` directory and update both the MCP server state and the UI state. The response SHALL include the count of tools found, their names, and any scan errors.

#### Scenario: New tool picked up after reload
- **WHEN** an agent creates `.potato/tools/new-tool/tool.toml` and calls `potato_reload_tools`
- **THEN** the new tool SHALL appear in subsequent `potato_list_tools` calls

#### Scenario: Removed tool dropped after reload
- **WHEN** a tool directory is deleted and `potato_reload_tools` is called
- **THEN** the tool SHALL no longer appear in `potato_list_tools`
