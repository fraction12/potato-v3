//! JSON-RPC 2.0 and MCP protocol types for the Potato inter-session MCP server.
//!
//! These types cover the stdio transport used by Claude Code's MCP integration.

use serde::{Deserialize, Serialize};
use serde_json::Value;

// ── JSON-RPC 2.0 error codes ──────────────────────────────────────────────────

pub const METHOD_NOT_FOUND: i64 = -32601;
pub const INVALID_PARAMS: i64 = -32602;
pub const INTERNAL_ERROR: i64 = -32603;

// ── JSON-RPC 2.0 core types ───────────────────────────────────────────────────

/// A JSON-RPC 2.0 request.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub id: Value,
    pub method: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

impl JsonRpcRequest {
    /// Create a new JSON-RPC 2.0 request with the given id, method, and optional params.
    pub fn new(id: impl Into<Value>, method: impl Into<String>, params: Option<Value>) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id: id.into(),
            method: method.into(),
            params,
        }
    }
}

/// A JSON-RPC 2.0 response.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

impl JsonRpcResponse {
    /// Build a successful JSON-RPC 2.0 response carrying the given result.
    pub fn success(id: impl Into<Value>, result: Value) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id: id.into(),
            result: Some(result),
            error: None,
        }
    }

    /// Build an error JSON-RPC 2.0 response with the given error object.
    pub fn error(id: impl Into<Value>, error: JsonRpcError) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id: id.into(),
            result: None,
            error: Some(error),
        }
    }
}

/// A JSON-RPC 2.0 error object.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct JsonRpcError {
    pub code: i64,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl JsonRpcError {
    /// Create an error with a numeric code and human-readable message.
    pub fn new(code: i64, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            data: None,
        }
    }

    /// Standard `-32601` error for an unrecognised method.
    pub fn method_not_found(method: &str) -> Self {
        Self::new(METHOD_NOT_FOUND, format!("Method not found: {method}"))
    }

    /// Standard `-32602` error for malformed or missing parameters.
    pub fn invalid_params(detail: &str) -> Self {
        Self::new(INVALID_PARAMS, format!("Invalid params: {detail}"))
    }

    /// Standard `-32603` error for unexpected server-side failures.
    pub fn internal_error(detail: &str) -> Self {
        Self::new(INTERNAL_ERROR, format!("Internal error: {detail}"))
    }
}

// ── MCP-specific types ────────────────────────────────────────────────────────

/// Parameters for the MCP `initialize` request.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct InitializeParams {
    pub protocol_version: String,
    pub capabilities: Value,
    pub client_info: ClientInfo,
}

/// Client identity sent during initialize.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ClientInfo {
    pub name: String,
    pub version: String,
}

/// Result returned from the MCP `initialize` handler.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct InitializeResult {
    pub protocol_version: String,
    pub capabilities: ServerCapabilities,
    pub server_info: ServerInfo,
}

/// Server capabilities advertised during initialize.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ServerCapabilities {
    pub tools: ToolsCapability,
}

/// Indicates the server supports tool listing.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ToolsCapability {
    pub list_changed: bool,
}

/// Server identity sent in initialize response.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ServerInfo {
    pub name: String,
    pub version: String,
}

/// Result returned from `tools/list`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ListToolsResult {
    pub tools: Vec<ToolInfo>,
}

/// Definition of a single MCP tool.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ToolInfo {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

/// Parameters for `tools/call`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CallToolParams {
    pub name: String,
    #[serde(default)]
    pub arguments: Value,
}

/// Result returned from `tools/call`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CallToolResult {
    pub content: Vec<ToolContent>,
    pub is_error: bool,
}

impl CallToolResult {
    /// Wrap a text payload as a successful tool result.
    pub fn success(text: impl Into<String>) -> Self {
        Self {
            content: vec![ToolContent::text(text)],
            is_error: false,
        }
    }

    /// Wrap a text payload as a failed tool result (`is_error = true`).
    pub fn failure(text: impl Into<String>) -> Self {
        Self {
            content: vec![ToolContent::text(text)],
            is_error: true,
        }
    }
}

/// A content item in a tool result.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolContent {
    #[serde(rename = "type")]
    pub content_type: String,
    pub text: String,
}

impl ToolContent {
    /// Create a text content item (the only content type currently used).
    pub fn text(s: impl Into<String>) -> Self {
        Self {
            content_type: "text".to_string(),
            text: s.into(),
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ── JsonRpcRequest ────────────────────────────────────────────────────────

    #[test]
    fn request_serializes_correctly() {
        let req = JsonRpcRequest::new(1, "initialize", Some(json!({"key": "value"})));
        let json = serde_json::to_string(&req).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["jsonrpc"], "2.0");
        assert_eq!(parsed["id"], 1);
        assert_eq!(parsed["method"], "initialize");
        assert_eq!(parsed["params"]["key"], "value");
    }

    #[test]
    fn request_deserializes_correctly() {
        let json = r#"{"jsonrpc":"2.0","id":42,"method":"tools/list","params":null}"#;
        // params: null gets deserialized as None (serde default for Option)
        let req: JsonRpcRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.jsonrpc, "2.0");
        assert_eq!(req.id, json!(42));
        assert_eq!(req.method, "tools/list");
    }

    #[test]
    fn request_without_params_omits_field() {
        let req = JsonRpcRequest::new(1, "tools/list", None);
        let json = serde_json::to_string(&req).unwrap();
        assert!(!json.contains("params"));
    }

    #[test]
    fn request_roundtrip() {
        let req = JsonRpcRequest::new(
            "req-1",
            "tools/call",
            Some(json!({"name": "potato_get_role", "arguments": {}})),
        );
        let serialized = serde_json::to_string(&req).unwrap();
        let deserialized: JsonRpcRequest = serde_json::from_str(&serialized).unwrap();
        assert_eq!(req, deserialized);
    }

    // ── JsonRpcResponse ───────────────────────────────────────────────────────

    #[test]
    fn response_success_serializes_correctly() {
        let resp = JsonRpcResponse::success(1, json!({"status": "ok"}));
        let json = serde_json::to_string(&resp).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["jsonrpc"], "2.0");
        assert_eq!(parsed["id"], 1);
        assert_eq!(parsed["result"]["status"], "ok");
        assert!(parsed.get("error").is_none() || parsed["error"].is_null());
    }

    #[test]
    fn response_error_serializes_correctly() {
        let err = JsonRpcError::new(METHOD_NOT_FOUND, "Method not found: foo");
        let resp = JsonRpcResponse::error(1, err);
        let json = serde_json::to_string(&resp).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["error"]["code"], METHOD_NOT_FOUND);
        assert!(parsed.get("result").is_none() || parsed["result"].is_null());
    }

    #[test]
    fn response_roundtrip() {
        let resp = JsonRpcResponse::success(42, json!({"tools": []}));
        let serialized = serde_json::to_string(&resp).unwrap();
        let deserialized: JsonRpcResponse = serde_json::from_str(&serialized).unwrap();
        assert_eq!(resp, deserialized);
    }

    // ── JsonRpcError ──────────────────────────────────────────────────────────

    #[test]
    fn error_codes_are_correct() {
        assert_eq!(METHOD_NOT_FOUND, -32601);
        assert_eq!(INVALID_PARAMS, -32602);
        assert_eq!(INTERNAL_ERROR, -32603);
    }

    #[test]
    fn error_constructors() {
        let e1 = JsonRpcError::method_not_found("foo");
        assert_eq!(e1.code, METHOD_NOT_FOUND);
        assert!(e1.message.contains("foo"));

        let e2 = JsonRpcError::invalid_params("missing field");
        assert_eq!(e2.code, INVALID_PARAMS);

        let e3 = JsonRpcError::internal_error("oops");
        assert_eq!(e3.code, INTERNAL_ERROR);
    }

    #[test]
    fn error_roundtrip() {
        let err = JsonRpcError {
            code: INTERNAL_ERROR,
            message: "boom".into(),
            data: Some(json!({"detail": "stack"})),
        };
        let serialized = serde_json::to_string(&err).unwrap();
        let deserialized: JsonRpcError = serde_json::from_str(&serialized).unwrap();
        assert_eq!(err, deserialized);
    }

    // ── InitializeParams ──────────────────────────────────────────────────────

    #[test]
    fn initialize_params_roundtrip() {
        let params = InitializeParams {
            protocol_version: "2024-11-05".into(),
            capabilities: json!({}),
            client_info: ClientInfo {
                name: "claude-code".into(),
                version: "1.0".into(),
            },
        };
        let serialized = serde_json::to_string(&params).unwrap();
        let deserialized: InitializeParams = serde_json::from_str(&serialized).unwrap();
        assert_eq!(params, deserialized);
    }

    #[test]
    fn initialize_params_field_names() {
        let params = InitializeParams {
            protocol_version: "2024-11-05".into(),
            capabilities: json!({}),
            client_info: ClientInfo {
                name: "test".into(),
                version: "1".into(),
            },
        };
        let json = serde_json::to_value(&params).unwrap();
        // camelCase via rename_all
        assert!(json.get("protocolVersion").is_some());
        assert!(json.get("clientInfo").is_some());
    }

    // ── InitializeResult ──────────────────────────────────────────────────────

    #[test]
    fn initialize_result_roundtrip() {
        let result = InitializeResult {
            protocol_version: "2024-11-05".into(),
            capabilities: ServerCapabilities {
                tools: ToolsCapability {
                    list_changed: false,
                },
            },
            server_info: ServerInfo {
                name: "potato".into(),
                version: "0.1.0".into(),
            },
        };
        let serialized = serde_json::to_string(&result).unwrap();
        let deserialized: InitializeResult = serde_json::from_str(&serialized).unwrap();
        assert_eq!(result, deserialized);
    }

    // ── ListToolsResult ───────────────────────────────────────────────────────

    #[test]
    fn list_tools_result_roundtrip() {
        let result = ListToolsResult {
            tools: vec![ToolInfo {
                name: "potato_get_role".into(),
                description: "Get this session's role".into(),
                input_schema: json!({"type": "object", "properties": {}}),
            }],
        };
        let serialized = serde_json::to_string(&result).unwrap();
        let deserialized: ListToolsResult = serde_json::from_str(&serialized).unwrap();
        assert_eq!(result, deserialized);
    }

    // ── CallToolParams ────────────────────────────────────────────────────────

    #[test]
    fn call_tool_params_roundtrip() {
        let params = CallToolParams {
            name: "potato_send_message".into(),
            arguments: json!({"to": "partner", "message": "hello"}),
        };
        let serialized = serde_json::to_string(&params).unwrap();
        let deserialized: CallToolParams = serde_json::from_str(&serialized).unwrap();
        assert_eq!(params, deserialized);
    }

    #[test]
    fn call_tool_params_default_arguments() {
        let json = r#"{"name": "potato_get_role"}"#;
        let params: CallToolParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.name, "potato_get_role");
        assert_eq!(params.arguments, json!(null));
    }

    // ── CallToolResult ────────────────────────────────────────────────────────

    #[test]
    fn call_tool_result_success() {
        let r = CallToolResult::success("All done");
        assert!(!r.is_error);
        assert_eq!(r.content[0].text, "All done");
        assert_eq!(r.content[0].content_type, "text");
    }

    #[test]
    fn call_tool_result_failure() {
        let r = CallToolResult::failure("Something went wrong");
        assert!(r.is_error);
        assert_eq!(r.content[0].text, "Something went wrong");
    }

    #[test]
    fn call_tool_result_roundtrip() {
        let r = CallToolResult::success("ok");
        let serialized = serde_json::to_string(&r).unwrap();
        let deserialized: CallToolResult = serde_json::from_str(&serialized).unwrap();
        assert_eq!(r, deserialized);
    }

    #[test]
    fn call_tool_result_field_name() {
        let r = CallToolResult::success("x");
        let json = serde_json::to_value(&r).unwrap();
        assert!(
            json.get("isError").is_some(),
            "isError should use camelCase"
        );
    }

    // ── ToolInfo ──────────────────────────────────────────────────────────────

    #[test]
    fn tool_info_roundtrip() {
        let tool = ToolInfo {
            name: "potato_claim_task".into(),
            description: "Claim a task".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "task_id": {"type": "string"},
                    "description": {"type": "string"}
                },
                "required": ["task_id"]
            }),
        };
        let serialized = serde_json::to_string(&tool).unwrap();
        let deserialized: ToolInfo = serde_json::from_str(&serialized).unwrap();
        assert_eq!(tool, deserialized);
    }

    #[test]
    fn tool_info_field_name() {
        let tool = ToolInfo {
            name: "x".into(),
            description: "y".into(),
            input_schema: json!({}),
        };
        let json = serde_json::to_value(&tool).unwrap();
        assert!(
            json.get("inputSchema").is_some(),
            "inputSchema should use camelCase"
        );
    }
}
