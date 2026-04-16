# Tool Factory — Proposal

## Problem

Potato V3 agents are limited to the built-in MCP tool surface. When a project needs custom integrations (query a database, check deployment status, run linters, fetch from an API), there's no mechanism for agents to use project-specific tools. Users must either fork the codebase or wait for upstream features.

Meanwhile, every multi-agent platform (Agent Orchestrator, claude-swarm, Composio) ships a closed tool set. None allow agents to extend the platform as they use it.

## Proposed Solution

A **convention-first tool factory** that lets users create custom tools as simple scripts in `.potato/tools/`. Each tool is a subfolder with a `tool.toml` manifest and an executable script. Potato discovers these tools at startup, exposes them via two MCP tools (`potato_list_tools` and `potato_run_tool`), and makes them available to all panes.

### Key Design Decisions

1. **User-directed, not autonomous.** Users prompt agents to build tools; agents don't create tools unprompted. Prevents tool sprawl.
2. **Convention over configuration.** `.potato/tools/<name>/tool.toml` + script. No registration API, no daemon, no database.
3. **Reload over watch.** V3 uses a `potato_reload_tools` MCP command (agents call after creating a tool) rather than filesystem watching. Three lines of code, ships today. Filesystem watcher deferred to V4.
4. **Shared by default.** One agent builds a tool, all panes can use it immediately after reload.
5. **Portable.** `.potato/tools/` travels with the codebase. Clone repo, get tools.

### Tool Categories (Examples)

- **Project integrations:** `fly-status`, `supabase-query`, `vercel-preview`, `sentry-errors`
- **Codebase intelligence:** `find-tests`, `check-migrations`, `lint-check`, `dep-graph`
- **Workflow automations:** `ship-it`, `review-prep`, `hotfix`, `changelog`
- **External context:** `docs-search`, `slack-latest`, `figma-spec`

## Scope

### In Scope
- `tool.toml` manifest schema (name, description, version, command, input schema, dependencies)
- `potato_list_tools` MCP tool — returns available custom tools with descriptions
- `potato_run_tool` MCP tool — executes a named tool with arguments, returns output
- `potato_reload_tools` MCP tool — rescans `.potato/tools/` directory
- Tool scanning at Potato startup (already landed in UI layer)
- Input validation against declared schema
- Timeout enforcement per tool invocation
- Error handling for missing dependencies, script failures, non-zero exits

### Out of Scope (V4+)
- Filesystem watcher for automatic hot-reload
- `potato_create_tool` scaffolding meta-tool
- Tool marketplace / community registry
- Sandboxing / permission gates for tool execution
- Tool versioning and dependency resolution
- Tool composition (chaining outputs as inputs)
- Audit logging of tool usage

## Success Criteria

1. A user can create a `.potato/tools/my-tool/tool.toml` + script, reload, and all agents can discover and run it via MCP.
2. Tool output is returned as structured MCP tool response.
3. Malformed manifests, missing scripts, and execution failures produce clear error messages.
4. Zero impact on existing MCP tool performance (tool scan is O(n) at startup/reload only).
