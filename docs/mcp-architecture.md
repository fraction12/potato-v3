# MCP Architecture

Potato uses the [Model Context Protocol](https://modelcontextprotocol.io/) (MCP) to let agents running in separate PTY sessions coordinate with each other. This document covers how the MCP system is structured, how requests flow through it, and the design decisions behind it.

## Overview

When Potato launches two agents side by side, each agent gets access to a set of MCP tools (`potato_send_message`, `potato_claim_role`, etc.) without any manual configuration. These tools let agents send messages, claim roles, share context, and coordinate tasks — all through Potato as the intermediary.

The MCP system has four layers:

```
Agent (Claude/Codex)           Agent (Claude/Codex)
       |                              |
   [stdio]                        [stdio]
       |                              |
  mcp-server                     mcp-server
  subprocess                     subprocess
       |                              |
   [Unix Domain Socket]          [Unix Domain Socket]
       |                              |
       +---------- McpBridge ---------+
                      |
              InterSessionState
            (in-memory shared state)
```

1. **Stdio** (MCP standard) -- Each agent talks JSON-RPC over stdin/stdout to its own `potato mcp-server` subprocess.
2. **UDS bridge** -- Each subprocess forwards requests over a Unix Domain Socket to the main Potato process.
3. **McpBridge** -- The main process accepts connections, dispatches requests to `McpServer`, and returns responses.
4. **InterSessionState** -- In-memory shared state (messages, roles, tasks, context) protected by `Arc<Mutex<>>`.

## Source Layout

All MCP code lives in `src/mcp/`:

| File | Lines | Purpose |
|------|-------|---------|
| `mod.rs` | 20 | Re-exports all submodules |
| `protocol.rs` | ~465 | JSON-RPC 2.0 types, MCP request/response structs |
| `server.rs` | ~411 | `McpServer` -- handles JSON-RPC requests for a single pane |
| `state.rs` | ~679 | `InterSessionState` -- shared mutable state, domain types |
| `tools.rs` | ~949 | Tool definitions (schemas) and dispatch logic |
| `bridge.rs` | ~567 | `McpBridge` -- UDS listener, connection handling, injection |
| `config_writer.rs` | ~328 | `.mcp.json` generation and cleanup |
| `injection.rs` | ~198 | Message injection into PTY sessions (formatting, safety) |

## Request Lifecycle

Here is the full path of a single MCP tool call, from agent to response:

```
1. Claude calls potato_send_message
   ↓
2. Claude's MCP client writes JSON-RPC to subprocess stdin:
   {"jsonrpc":"2.0","id":1,"method":"tools/call","params":{...}}
   ↓
3. mcp-server subprocess (potato mcp-server) reads stdin
   - Reads POTATO_PANE_ID and POTATO_SOCKET from environment
   - Wraps request: {"pane_id": 0, "request": "<json-rpc-string>"}
   - Connects to Unix Domain Socket at POTATO_SOCKET
   - Sends wrapped request over UDS
   ↓
4. McpBridge accepts connection, reads BridgeRequest
   - Parses pane_id and JSON-RPC request string
   - Creates McpServer instance for that pane
   - Calls McpServer::handle_request()
   ↓
5. McpServer routes by method:
   - "initialize"  → protocol handshake
   - "initialized" → acknowledgment (no-op)
   - "tools/list"  → returns all 8 tool definitions
   - "tools/call"  → dispatches to tools::handle_tool_call()
   ↓
6. handle_tool_call() acquires Mutex on InterSessionState
   - Validates parameters
   - Mutates state (sends message, claims role, etc.)
   - Returns CallToolResult (success or failure)
   ↓
7. Response flows back:
   McpServer → BridgeResponse → UDS → subprocess stdout → Claude
   {"jsonrpc":"2.0","id":1,"result":{...}}
```

**Special case: `potato_send_message`** -- After dispatching, the bridge also detects send_message calls and queues an `InjectRequest` to deliver the message text into the target agent's PTY. See [Message Injection](#message-injection) below.

## MCP Tools Reference

All tools are defined in `src/mcp/tools.rs`. Each tool returns a `CallToolResult` with `content` (text) and `isError` (boolean).

### potato_send_message

Send a message to another agent session.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `message` | string | yes | The message content |
| `to` | string | no | `"partner"` (default) or a pane ID like `"1"` |
| `priority` | string | no | `"normal"` (default) or `"urgent"` |

Urgent messages trigger immediate PTY injection. Partner resolution picks the first registered pane that isn't the sender.

### potato_get_messages

Check inbox for unread messages.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `mark_read` | boolean | no | Whether to mark messages as read (default: `true`) |

Returns all unread messages with sender pane ID, content, priority, and timestamp.

### potato_get_partner_status

Get status of all other panes.

No parameters. Returns each partner's pane ID, role, and unread message count.

**Note:** Only panes that have claimed a role appear in the results. Panes without a claimed role are not visible to this tool (the implementation iterates `self.roles`, not `self.known_panes`).

### potato_shared_context

CRUD operations on a shared key-value store. Values are arbitrary JSON.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `op` | string | yes | `"get"`, `"set"`, `"delete"`, or `"list"` |
| `key` | string | for get/set/delete | The context key |
| `value` | any | for set | Any JSON value to store |

### potato_claim_task

Claim exclusive ownership of a task.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `task_id` | string | yes | Unique task identifier |
| `description` | string | no | What the task is about |

Returns success if unclaimed or already held by the same pane. Returns `AlreadyClaimed` with the holder's pane ID and timestamp if another pane holds it. Re-claiming by the same pane is idempotent (updates description).

### potato_release_task

Release a previously claimed task.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `task_id` | string | yes | Task to release |

Only the pane that claimed the task can release it.

### potato_claim_role

Claim a named role for this session.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `role` | string | yes | Role name (e.g., `"architect"`) |
| `description` | string | no | What this role does |

Role names are case-insensitive and unique across panes. If another pane already holds the role, the claim is rejected. Re-claiming by the same pane updates the description.

### potato_get_role

Get your own role and see all claimed roles.

No parameters. Returns the caller's assigned role and a list of all pane-role assignments.

## Inter-Session State

`InterSessionState` (`src/mcp/state.rs`) is the single source of truth for all coordination data. It lives in the main Potato process, protected by `Arc<Mutex<>>`.

```rust
pub struct InterSessionState {
    pub inboxes: HashMap<u64, VecDeque<InterMessage>>,
    pub shared_context: HashMap<String, Value>,
    pub task_board: HashMap<String, TaskClaim>,
    pub roles: HashMap<u64, PaneRole>,
    pub known_panes: Vec<u64>,
}
```

### State Characteristics

- **In-memory only** -- Not persisted to disk. State lives as long as the Potato process.
- **Thread-safe** -- All access goes through `Mutex`. Mutations are atomic.
- **Poison-resistant** -- Mutex poisoning is caught and returned as tool failures, never panics.
- **Pane lifecycle aware** -- Panes register on spawn, unregister on close. Closing a pane does not clear its messages or roles (allows posthumous inspection).

### Key Domain Types

```rust
pub struct InterMessage {
    pub from_pane: u64,
    pub content: String,
    pub priority: MessagePriority,    // Normal | Urgent
    pub timestamp: DateTime<Utc>,
    pub read: bool,
}

pub struct PaneRole {
    pub name: String,
    pub description: String,
}

pub struct TaskClaim {
    pub task_id: String,
    pub description: String,
    pub claimed_by: u64,
    pub claimed_at: DateTime<Utc>,
}
```

## Message Injection

When an agent sends a message via `potato_send_message`, the response confirms delivery to the inbox. But for the target agent to actually *see* the message, Potato injects it directly into the target's PTY input.

### Two-Phase Delivery

Injection happens in two phases to avoid a race condition with Claude's Ink-based terminal renderer:

**Phase 1 -- Write text immediately:**
The bridge queues an `InjectRequest`. The main event loop drains the queue and writes the formatted message text into the target PTY's stdin.

**Phase 2 -- Send Enter after delay:**
A `PendingEnter` is queued with a 5-tick delay (~83ms at ~60Hz tick rate). After the delay, `\r` (Enter) is written to the PTY, submitting the message to the agent.

```rust
pub struct InjectRequest {
    pub from_pane: u64,
    pub from_role: Option<String>,
    pub to_pane: u64,
    pub content: String,
}
```

### Format

Injected text appears as:
```
[Potato: Pane 0 (architect)] Hey, I've finished the plan.
```

The `format_notification()` function sanitizes all control characters (newlines, ANSI escapes, null bytes) and collapses consecutive double spaces into single spaces, producing a single line of clean, printable text.

### Safety Guards

- **Approval pending** -- If the target pane has a tool approval dialog active, injection is skipped. This prevents accidentally confirming or denying a tool call.
- **No PTY handle** -- If the target pane has no PTY attached at all, injection returns an error.
- **Dead PTY** -- If the target PTY process has exited, injection is skipped.
- **I/O errors** -- Write failures are logged and reported, never cause panics.

## UDS Bridge

`McpBridge` (`src/mcp/bridge.rs`) is the Unix Domain Socket listener that connects per-pane stdio servers to the shared state.

### Wire Protocol

Line-delimited JSON over UDS:

```
→ {"pane_id": 0, "request": "{\"jsonrpc\":\"2.0\",\"id\":1,...}"}
← {"response": "{\"jsonrpc\":\"2.0\",\"id\":1,...}"}
```

### Lifecycle

1. **Startup** -- `McpBridge::start()` binds to `/tmp/potato-{pid}.sock`. Stale socket files from previous crashes are removed automatically.
2. **Connections** -- Each per-pane `potato mcp-server` subprocess connects independently. Connections are handled concurrently via `tokio::spawn`.
3. **Shutdown** -- Socket file is removed on `McpBridge::shutdown()` or `Drop`.

## Configuration (.mcp.json)

Potato writes a `.mcp.json` file in the project root so agents discover the MCP server automatically.

### Format

```json
{
  "mcpServers": {
    "potato": {
      "command": "potato",
      "args": ["mcp-server"]
    }
  }
}
```

A single shared entry works for all panes because each subprocess inherits `POTATO_PANE_ID` and `POTATO_SOCKET` from its parent PTY environment. The subprocess knows which pane it represents without per-pane config.

### Lifecycle

| Event | Action |
|-------|--------|
| Pane spawns | `write_mcp_config()` creates/updates `.mcp.json` |
| All panes close | `remove_mcp_config()` removes Potato entries |
| User has other MCP servers | Preserved -- only `potato*` entries are touched |

Legacy per-pane entries (`potato-0`, `potato-1`) are cleaned up automatically.

### Environment Variables

Set per-pane when spawning the agent PTY:

| Variable | Example | Purpose |
|----------|---------|---------|
| `POTATO_PANE_ID` | `0` | Identifies which pane this agent occupies |
| `POTATO_SOCKET` | `/tmp/potato-12345.sock` | UDS path to reach the bridge |

## Protocol Details

### JSON-RPC 2.0

The MCP system uses standard JSON-RPC 2.0 with these methods:

| Method | Direction | Purpose |
|--------|-----------|---------|
| `initialize` | agent -> server | Protocol handshake, capability exchange |
| `initialized` | agent -> server | Client acknowledgment (returns `{}`) |
| `tools/list` | agent -> server | Enumerate available tools and schemas |
| `tools/call` | agent -> server | Execute a tool |

Protocol version: `2024-11-05`. Server identifies as `potato` with the cargo package version.

### Error Handling

| Scenario | Behavior |
|----------|----------|
| Malformed bridge request | JSON-RPC error `-32700` (Parse Error) |
| Unknown method | JSON-RPC error `-32601` (Method Not Found) |
| Invalid parameters | JSON-RPC error `-32602` (Invalid Params) |
| Unknown tool name | `CallToolResult` with `isError: true` (not a JSON-RPC error) |
| Missing required field | `CallToolResult` with `isError: true` and field name |
| Mutex poisoned | `CallToolResult` with `isError: true`, logged as error |
| UDS connection lost | Subprocess exits gracefully, agent session ends |

Tool-level failures use `isError: true` in the result (not JSON-RPC error codes). This follows MCP convention -- the protocol succeeded, but the tool operation failed.

## Design Decisions

### Why a UDS bridge instead of in-process?

Each agent's MCP client expects a stdio subprocess (`command` + `args` in `.mcp.json`). Potato runs `potato mcp-server` as that subprocess, which connects to the main process over UDS. This keeps:
- The shared state in one place (main process)
- The transport standard-compliant (stdio for MCP, UDS for internal routing)
- Multiple panes able to share state without IPC complexity

### Why Mutex over channels?

`InterSessionState` uses `Arc<Mutex<>>` instead of message-passing channels. The state is small, mutations are fast, and the synchronous access pattern maps naturally to the request-response model. Mutex poisoning is handled gracefully.

### Why in-memory state?

Coordination state (messages, roles, tasks, context) is ephemeral by design. It lives only while Potato is running. Sessions are meant to be focused collaboration windows, not persistent databases. Session history is persisted separately via SQLite.

### Why two-phase message injection?

Claude Code uses an Ink-based terminal renderer. If text and Enter are written simultaneously, the Enter keypress can be lost while Ink is re-rendering. The 5-tick (~83ms) delay between text injection and Enter submission ensures the agent's renderer has time to process the input.

### Why a single .mcp.json entry?

Earlier versions used per-pane entries (`potato-0`, `potato-1`). The current design uses one shared `potato` entry with per-pane environment variables (`POTATO_PANE_ID`, `POTATO_SOCKET`). This is cleaner, scales to any number of panes, and avoids config file churn.

### Why skip injection during approval?

Claude Code prompts the user to approve or deny tool calls. If a message injection fires during this prompt, the injected text could accidentally confirm or deny the approval. Potato checks `approval_pending` state and defers injection until the prompt clears.
