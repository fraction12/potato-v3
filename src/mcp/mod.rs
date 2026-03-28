//! Potato inter-session MCP layer.
//!
//! Enables two Claude PTY sessions to communicate in real-time through
//! a shared in-process state exposed via the MCP stdio protocol.
//!
//! # Architecture
//! - `protocol` — JSON-RPC 2.0 and MCP-specific types (serde)
//! - `state`    — `InterSessionState` holding inboxes, shared context, task board, roles
//! - `tools`    — Tool definitions and dispatch (`handle_tool_call`)
//! - `server`   — `McpServer` that processes JSON-RPC requests from a pane
//! - `config_writer` — Dynamic `.mcp.json` lifecycle management

pub mod bridge;
pub mod config_writer;
pub mod protocol;
pub mod server;
pub mod state;
pub mod tools;
