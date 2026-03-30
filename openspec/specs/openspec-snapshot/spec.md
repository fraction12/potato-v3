# openspec-snapshot Specification

## Purpose
TBD - created by archiving change openspec-cli-integration. Update Purpose after archive.
## Requirements
### Requirement: OpenSpec CLI detection
The system SHALL detect whether the `openspec` CLI binary is available on the system PATH. If `openspec` is not found, the snapshot SHALL be empty and the UI SHALL display a "openspec not found" indicator.

#### Scenario: CLI available
- **WHEN** the `openspec` binary exists on PATH and returns successfully
- **THEN** `OpenSpecSnapshot::capture()` SHALL populate the snapshot with change and artifact data

#### Scenario: CLI not available
- **WHEN** the `openspec` binary is not found on PATH
- **THEN** `OpenSpecSnapshot::capture()` SHALL return a default empty snapshot without panicking

### Requirement: Change list capture
The system SHALL capture a list of all OpenSpec changes by executing `openspec list --json`. Each change entry SHALL include: name, completed task count, total task count, last modified timestamp, and status.

#### Scenario: Project has active changes
- **WHEN** `openspec list --json` returns a JSON array of changes
- **THEN** the snapshot SHALL contain a `Vec<ChangeInfo>` with one entry per change, sorted by most recently modified first

#### Scenario: No changes exist
- **WHEN** `openspec list --json` returns an empty array
- **THEN** the snapshot SHALL have an empty changes list and `is_active` SHALL be `false`

#### Scenario: CLI command fails
- **WHEN** `openspec list --json` returns a non-zero exit code
- **THEN** the snapshot SHALL be empty and the error SHALL be logged via `tracing::warn`

### Requirement: Per-change artifact status capture
The system SHALL capture artifact status for each in-progress change by executing `openspec status --change <name> --json`. Artifact data SHALL include: artifact id, output path, status (ready/blocked/done), and missing dependencies.

#### Scenario: Change has artifacts in various states
- **WHEN** `openspec status --change <name> --json` returns artifact data
- **THEN** each `ChangeInfo` SHALL contain a `Vec<ArtifactInfo>` reflecting the artifact dependency graph

#### Scenario: Capture is capped at 5 changes
- **WHEN** more than 5 in-progress changes exist
- **THEN** only the 5 most recently modified changes SHALL have artifact status fetched

### Requirement: Periodic snapshot refresh
The system SHALL refresh the OpenSpec snapshot periodically in the main event loop using a tick counter, matching the existing git refresh cadence (~30 seconds).

#### Scenario: Automatic refresh
- **WHEN** 120 ticks (at 250ms per tick) have elapsed since the last refresh
- **THEN** a new `OpenSpecSnapshot::capture()` SHALL be called and stored in `AppState`

#### Scenario: Manual refresh via F5
- **WHEN** the user presses F5
- **THEN** the OpenSpec snapshot SHALL be immediately recaptured alongside the git snapshot

### Requirement: Sidebar panel displays OpenSpec data
The sidebar panel previously titled "Tasks" SHALL be renamed to "OpenSpec". It SHALL display a per-change summary showing change name and task progress (completed/total).

#### Scenario: Active changes with tasks
- **WHEN** the snapshot contains changes with task data
- **THEN** the panel SHALL render one line per change showing: change name (truncated to fit) and task fraction (e.g., "2/7")

#### Scenario: No OpenSpec data
- **WHEN** the snapshot is empty (CLI not found or no changes)
- **THEN** the panel SHALL display an appropriate empty-state message

#### Scenario: Change with all artifacts done
- **WHEN** a change has all artifacts in "done" status
- **THEN** a completion indicator SHALL be shown alongside the change name

### Requirement: MCP task list compatibility
The MCP tool `potato_list_tasks` SHALL continue to return task-level data derived from the new snapshot. The response format SHALL remain compatible with existing agent consumers.

#### Scenario: Agent calls potato_list_tasks
- **WHEN** an agent invokes the `potato_list_tasks` MCP tool
- **THEN** the tool SHALL return task data derived from the OpenSpec snapshot changes (id-derived from change names, status from change status, task counts)

### Requirement: Non-fatal snapshot capture
All CLI calls within `OpenSpecSnapshot::capture()` SHALL be non-fatal. Failures in individual commands SHALL NOT prevent the snapshot from being created — partial data is preferred over no data.

#### Scenario: One status call fails
- **WHEN** `openspec list --json` succeeds but `openspec status --change X --json` fails for one change
- **THEN** the snapshot SHALL include that change from the list data but with an empty artifacts vector

