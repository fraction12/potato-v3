//! MCP server request handler for the Potato inter-session layer.
//!
//! `McpServer` processes JSON-RPC 2.0 requests from a single pane's Claude session,
//! dispatching them to the shared `InterSessionState` via the tools layer.

use std::sync::{Arc, Mutex};

use serde_json::{Value, json};

use crate::mcp::protocol::{
    CallToolParams, INVALID_PARAMS, InitializeParams, InitializeResult, JsonRpcError,
    JsonRpcRequest, JsonRpcResponse, ListToolsResult, METHOD_NOT_FOUND, PARSE_ERROR,
    ServerCapabilities, ServerInfo, ToolsCapability,
};
use crate::mcp::state::InterSessionState;
use crate::mcp::tools::{handle_tool_call, tool_definitions};

// ── Server version ────────────────────────────────────────────────────────────

pub const PROTOCOL_VERSION: &str = "2024-11-05";
pub const SERVER_NAME: &str = "potato";
pub const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");

// ── McpServer ─────────────────────────────────────────────────────────────────

/// Handles MCP JSON-RPC requests for a single pane's Claude session.
pub struct McpServer {
    /// Which pane this server instance represents.
    pub pane_id: u64,
    /// Shared state accessible to all pane servers.
    pub state: Arc<Mutex<InterSessionState>>,
}

impl McpServer {
    /// Create a new MCP server instance for `pane_id`, backed by shared `state`.
    pub fn new(pane_id: u64, state: Arc<Mutex<InterSessionState>>) -> Self {
        Self { pane_id, state }
    }

    /// Process a JSON-RPC request string and return a JSON-RPC response string.
    ///
    /// Never panics — all errors are returned as JSON-RPC error responses.
    pub fn handle_request(&self, json_str: &str) -> String {
        // Parse incoming JSON.
        let request: JsonRpcRequest = match serde_json::from_str(json_str) {
            Ok(r) => r,
            Err(e) => {
                // Can't determine id if parsing failed — use null.
                let resp = JsonRpcResponse::error(
                    Value::Null,
                    JsonRpcError::new(PARSE_ERROR, format!("Parse error: {e}")),
                );
                return serde_json::to_string(&resp).unwrap_or_default();
            }
        };

        let id = request.id.clone();

        let response = match request.method.as_str() {
            "initialize" => self.handle_initialize(&id, request.params.as_ref()),
            "initialized" => {
                // Notification — per JSON-RPC 2.0, no response should be sent.
                return String::new();
            }
            "tools/list" => self.handle_list_tools(&id),
            "tools/call" => self.handle_call_tool(&id, request.params.as_ref()),
            method => JsonRpcResponse::error(id, JsonRpcError::method_not_found(method)),
        };

        serde_json::to_string(&response).unwrap_or_default()
    }

    // ── Method handlers ───────────────────────────────────────────────────────

    /// Serialize a result to JSON and wrap in a `JsonRpcResponse`.
    fn json_response(id: &Value, result: impl serde::Serialize) -> JsonRpcResponse {
        match serde_json::to_value(result) {
            Ok(v) => JsonRpcResponse::success(id.clone(), v),
            Err(e) => {
                JsonRpcResponse::error(id.clone(), JsonRpcError::internal_error(&e.to_string()))
            }
        }
    }

    fn handle_initialize(&self, id: &Value, params: Option<&Value>) -> JsonRpcResponse {
        // Params are informational — we accept any client.
        let _params: Option<InitializeParams> =
            params.and_then(|p| serde_json::from_value(p.clone()).ok());

        let result = InitializeResult {
            protocol_version: PROTOCOL_VERSION.to_string(),
            capabilities: ServerCapabilities {
                tools: ToolsCapability {
                    list_changed: false,
                },
            },
            server_info: ServerInfo {
                name: SERVER_NAME.to_string(),
                version: SERVER_VERSION.to_string(),
            },
        };

        Self::json_response(id, result)
    }

    fn handle_list_tools(&self, id: &Value) -> JsonRpcResponse {
        let result = ListToolsResult {
            tools: tool_definitions(),
        };
        Self::json_response(id, result)
    }

    fn handle_call_tool(&self, id: &Value, params: Option<&Value>) -> JsonRpcResponse {
        let params_val = match params {
            Some(p) => p,
            None => {
                return JsonRpcResponse::error(
                    id.clone(),
                    JsonRpcError::invalid_params("tools/call requires params"),
                );
            }
        };

        let call_params: CallToolParams = match serde_json::from_value(params_val.clone()) {
            Ok(p) => p,
            Err(e) => {
                return JsonRpcResponse::error(
                    id.clone(),
                    JsonRpcError::invalid_params(&e.to_string()),
                );
            }
        };

        let tool_result = handle_tool_call(
            &call_params.name,
            &call_params.arguments,
            self.pane_id,
            &self.state,
        );

        Self::json_response(id, tool_result)
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mcp::state::PaneRole;
    use serde_json::json;

    fn make_server(pane_id: u64) -> McpServer {
        let state = Arc::new(Mutex::new(InterSessionState::new()));
        McpServer::new(pane_id, state)
    }

    fn make_server_with_state() -> (McpServer, McpServer, Arc<Mutex<InterSessionState>>) {
        let state = Arc::new(Mutex::new(InterSessionState::new()));
        {
            let mut st = state.lock().unwrap();
            st.set_role(
                0,
                PaneRole {
                    name: "architect".into(),
                    description: "Designs".into(),
                },
            );
            st.set_role(
                1,
                PaneRole {
                    name: "implementer".into(),
                    description: "Builds".into(),
                },
            );
        }
        let server0 = McpServer::new(0, Arc::clone(&state));
        let server1 = McpServer::new(1, Arc::clone(&state));
        (server0, server1, state)
    }

    fn parse_response(json_str: &str) -> Value {
        serde_json::from_str(json_str).expect("Response should be valid JSON")
    }

    // ── Parse errors ──────────────────────────────────────────────────────────

    #[test]
    fn malformed_json_returns_error() {
        let server = make_server(0);
        let resp = parse_response(&server.handle_request("not json at all"));
        assert!(resp["error"].is_object());
        assert_eq!(resp["id"], Value::Null);
    }

    // ── initialize ────────────────────────────────────────────────────────────

    #[test]
    fn initialize_returns_protocol_version() {
        let server = make_server(0);
        let req = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": {"name": "claude-code", "version": "1.0"}
            }
        });
        let resp = parse_response(&server.handle_request(&req.to_string()));
        assert_eq!(resp["result"]["protocolVersion"], PROTOCOL_VERSION);
        assert_eq!(resp["result"]["serverInfo"]["name"], SERVER_NAME);
    }

    #[test]
    fn initialize_returns_tools_capability() {
        let server = make_server(0);
        let req = json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {"name": "test", "version": "1"}
        }});
        let resp = parse_response(&server.handle_request(&req.to_string()));
        assert!(resp["result"]["capabilities"]["tools"].is_object());
    }

    #[test]
    fn initialize_preserves_request_id() {
        let server = make_server(0);
        let req = json!({"jsonrpc": "2.0", "id": 99, "method": "initialize", "params": {
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {"name": "test", "version": "1"}
        }});
        let resp = parse_response(&server.handle_request(&req.to_string()));
        assert_eq!(resp["id"], 99);
        assert!(resp["error"].is_null());
    }

    // ── tools/list ────────────────────────────────────────────────────────────

    #[test]
    fn tools_list_returns_all_tools() {
        let server = make_server(0);
        let req = json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"});
        let resp = parse_response(&server.handle_request(&req.to_string()));
        let tools = &resp["result"]["tools"];
        assert!(tools.is_array());
        assert!(tools.as_array().unwrap().len() >= 6);
    }

    #[test]
    fn tools_list_tools_have_name_description_inputschema() {
        let server = make_server(0);
        let req = json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"});
        let resp = parse_response(&server.handle_request(&req.to_string()));
        for tool in resp["result"]["tools"].as_array().unwrap() {
            assert!(tool["name"].is_string(), "tool missing name");
            assert!(tool["description"].is_string(), "tool missing description");
            assert!(tool["inputSchema"].is_object(), "tool missing inputSchema");
        }
    }

    // ── tools/call ────────────────────────────────────────────────────────────

    #[test]
    fn tools_call_get_role() {
        let (server0, _server1, _state) = make_server_with_state();
        let req = json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "tools/call",
            "params": {"name": "potato_get_role", "arguments": {}}
        });
        let resp = parse_response(&server0.handle_request(&req.to_string()));
        assert!(
            resp["error"].is_null(),
            "Unexpected error: {}",
            resp["error"]
        );
        let content_text = resp["result"]["content"][0]["text"].as_str().unwrap();
        assert!(content_text.contains("architect"));
    }

    #[test]
    fn tools_call_send_then_get_messages() {
        let (server0, server1, _state) = make_server_with_state();

        // Pane 0 sends a message.
        let send_req = json!({
            "jsonrpc": "2.0",
            "id": 4,
            "method": "tools/call",
            "params": {
                "name": "potato_send_message",
                "arguments": {"message": "hey pane 1", "to": "1"}
            }
        });
        let send_resp = parse_response(&server0.handle_request(&send_req.to_string()));
        assert!(send_resp["error"].is_null());

        // Pane 1 reads the message.
        let get_req = json!({
            "jsonrpc": "2.0",
            "id": 5,
            "method": "tools/call",
            "params": {"name": "potato_get_messages", "arguments": {}}
        });
        let get_resp = parse_response(&server1.handle_request(&get_req.to_string()));
        assert!(get_resp["error"].is_null());
        let text = get_resp["result"]["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("hey pane 1"));
    }

    #[test]
    fn tools_call_unknown_tool_returns_is_error_true() {
        let server = make_server(0);
        let req = json!({
            "jsonrpc": "2.0",
            "id": 6,
            "method": "tools/call",
            "params": {"name": "potato_nonexistent", "arguments": {}}
        });
        let resp = parse_response(&server.handle_request(&req.to_string()));
        // tools/call returns a result (not a JSON-RPC error) with isError:true
        assert!(resp["result"]["isError"].as_bool().unwrap_or(false));
    }

    #[test]
    fn tools_call_missing_params_returns_error() {
        let server = make_server(0);
        let req = json!({"jsonrpc": "2.0", "id": 7, "method": "tools/call"});
        let resp = parse_response(&server.handle_request(&req.to_string()));
        assert!(resp["error"].is_object());
    }

    // ── Unknown method ────────────────────────────────────────────────────────

    #[test]
    fn unknown_method_returns_method_not_found() {
        let server = make_server(0);
        let req = json!({"jsonrpc": "2.0", "id": 8, "method": "some/unknown"});
        let resp = parse_response(&server.handle_request(&req.to_string()));
        assert_eq!(resp["error"]["code"], METHOD_NOT_FOUND);
    }

    // ── Full round-trip: initialize → list → call ─────────────────────────────

    #[test]
    fn full_mcp_handshake_roundtrip() {
        let server = make_server(0);

        // 1. Initialize
        let init_req = json!({
            "jsonrpc": "2.0",
            "id": "init-1",
            "method": "initialize",
            "params": {
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": {"name": "claude-code", "version": "1.0"}
            }
        });
        let init_resp = parse_response(&server.handle_request(&init_req.to_string()));
        assert!(init_resp["error"].is_null());
        assert_eq!(init_resp["result"]["protocolVersion"], PROTOCOL_VERSION);

        // 2. List tools
        let list_req = json!({"jsonrpc": "2.0", "id": "list-1", "method": "tools/list"});
        let list_resp = parse_response(&server.handle_request(&list_req.to_string()));
        assert!(list_resp["error"].is_null());
        let tools = list_resp["result"]["tools"].as_array().unwrap();
        assert!(!tools.is_empty());

        // 3. Call a tool
        let call_req = json!({
            "jsonrpc": "2.0",
            "id": "call-1",
            "method": "tools/call",
            "params": {"name": "potato_get_role", "arguments": {}}
        });
        let call_resp = parse_response(&server.handle_request(&call_req.to_string()));
        assert!(call_resp["error"].is_null());
        assert!(call_resp["result"]["content"].is_array());
    }

    #[test]
    fn claim_and_release_task_cycle() {
        let server = make_server(0);
        let server1 = McpServer::new(1, Arc::clone(&server.state));

        // Pane 0 claims task.
        let claim_req = json!({
            "jsonrpc": "2.0", "id": 1, "method": "tools/call",
            "params": {"name": "potato_claim_task", "arguments": {"task_id": "feat-auth"}}
        });
        let claim_resp = parse_response(&server.handle_request(&claim_req.to_string()));
        let claim_text = &claim_resp["result"]["content"][0]["text"];
        assert!(claim_text.as_str().unwrap().contains("true"));

        // Pane 1 tries to claim same task — should fail.
        let claim2_resp = parse_response(&server1.handle_request(&claim_req.to_string()));
        let claim2_text = claim2_resp["result"]["content"][0]["text"]
            .as_str()
            .unwrap();
        let parsed: Value = serde_json::from_str(claim2_text).unwrap();
        assert_eq!(parsed["claimed"], false);

        // Pane 0 releases.
        let release_req = json!({
            "jsonrpc": "2.0", "id": 2, "method": "tools/call",
            "params": {"name": "potato_release_task", "arguments": {"task_id": "feat-auth"}}
        });
        let release_resp = parse_response(&server.handle_request(&release_req.to_string()));
        let release_text = release_resp["result"]["content"][0]["text"]
            .as_str()
            .unwrap();
        assert!(release_text.contains("Released"));

        // Pane 1 can now claim.
        let claim3_resp = parse_response(&server1.handle_request(&claim_req.to_string()));
        let claim3_text = claim3_resp["result"]["content"][0]["text"]
            .as_str()
            .unwrap();
        let parsed3: Value = serde_json::from_str(claim3_text).unwrap();
        assert_eq!(parsed3["claimed"], true);
    }
}
