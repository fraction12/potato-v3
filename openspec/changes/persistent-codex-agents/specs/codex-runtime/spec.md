## ADDED Requirements

### Requirement: Persistent Codex agents SHALL use native Codex thread continuity

An active Codex agent in Potato SHALL preserve continuity using Codex’s native thread/session id. The first turn SHALL bind the native id from Codex output, and subsequent turns to that same agent SHALL resume the same native Codex thread.

#### Scenario: First turn binds native Codex thread id
- **WHEN** Potato sends the first message to a new Codex agent
- **THEN** Potato SHALL execute Codex in JSON mode, capture the `thread.started` event, and persist the returned native thread id for that agent

#### Scenario: Subsequent turn resumes the same Codex thread
- **WHEN** Potato sends another message to an existing Codex agent with a known native thread id
- **THEN** Potato SHALL send that turn with `codex exec resume <thread_id> --json` rather than starting a fresh thread

### Requirement: Potato-local pane identity SHALL remain separate from provider-native session identity

Potato SHALL store a local pane/session identity separately from the provider-native session identity used by Codex. Local pane selection and routing SHALL NOT depend on fabricated or placeholder Codex thread ids.

#### Scenario: Local pane id differs from native Codex thread id
- **WHEN** a new Codex pane is created before Codex returns `thread.started`
- **THEN** Potato SHALL keep using its own local pane/session id for UI and routing, and SHALL only attach the Codex-native id after it is returned by Codex

#### Scenario: Resume uses native id, not local id
- **WHEN** Potato resumes an active Codex agent
- **THEN** the resume command SHALL use the stored native Codex thread id, not the local pane/session id

### Requirement: Codex orchestration SHALL default to exec-backed JSON mode

Potato SHALL use non-interactive Codex JSON execution as the default orchestration backend for active Codex agents. Interactive native Codex mode SHALL only be entered through an explicit takeover action.

#### Scenario: Default Codex spawn uses exec-backed mode
- **WHEN** the user creates or messages a Codex agent through Potato’s normal orchestration flow
- **THEN** Potato SHALL use the exec-backed JSON path, not an embedded interactive Codex TUI, as the primary runtime

#### Scenario: Interactive Codex requires explicit takeover
- **WHEN** the user wants the native Codex UI
- **THEN** Potato SHALL require an explicit takeover/open-native action rather than using interactive Codex as the default pane mode

### Requirement: Exec-backed Codex agents SHALL remain visible and messageable in Potato

Potato SHALL render enough structured state for an exec-backed Codex agent to behave like an active agent in the roster. This includes current run state, recent transcript, and recent tool activity.

#### Scenario: Idle Codex agent remains visible after turn completion
- **WHEN** a Codex turn completes successfully
- **THEN** the Codex agent SHALL remain in the active roster with status `idle`, preserving its native thread id for later messages

#### Scenario: Running Codex agent shows structured state
- **WHEN** a Codex agent is in the middle of an exec-backed turn
- **THEN** Potato SHALL show that agent as running and render structured transcript/tool activity without requiring a raw terminal viewport

### Requirement: Potato SHALL prevent overlapping turns on one Codex thread

Potato SHALL not run multiple simultaneous turns against the same Codex native thread.

#### Scenario: Second send while agent is running
- **WHEN** the user sends another message to a Codex agent whose turn is still running
- **THEN** Potato SHALL reject the send with a clear reason or enqueue it explicitly, but SHALL NOT start a second overlapping run against the same thread

### Requirement: Codex MCP availability SHALL be checked explicitly

Potato SHALL detect whether Codex can reach `potato mcp-server` through Codex’s MCP configuration path. If Potato MCP is unavailable to Codex, Potato SHALL show actionable setup guidance instead of silently assuming `.mcp.json` is sufficient.

#### Scenario: Codex MCP registration missing
- **WHEN** Potato creates or inspects a Codex agent and Codex does not have a usable `potato` MCP server registration
- **THEN** Potato SHALL surface a warning describing the missing registration and the corrective setup step

#### Scenario: Codex MCP registration present
- **WHEN** Codex has a working `potato` MCP server registration
- **THEN** Potato SHALL mark MCP status as available for that Codex agent
