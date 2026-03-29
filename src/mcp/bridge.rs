//! Unix domain socket bridge between MCP server subprocesses and the in-process
//! `InterSessionState`.
//!
//! # Wire protocol
//!
//! Every line sent from an `mcp-server` subprocess to the bridge is a JSON object:
//!
//! ```json
//! {"pane_id": 0, "request": "<json-rpc-string>"}
//! ```
//!
//! The bridge dispatches the request through `McpServer::handle_request()` and
//! writes back a single-line JSON response:
//!
//! ```json
//! {"response": "<json-rpc-string>"}
//! ```
//!
//! Multiple simultaneous connections are supported — one per Claude pane.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixListener;
use tokio::task::JoinHandle;

use tokio::sync::mpsc::UnboundedSender;

use crate::mcp::injection::InjectRequest;
use crate::mcp::server::McpServer;
use crate::mcp::state::InterSessionState;

// ── Wire types ─────────────────────────────────────────────────────────────────

/// A single line sent from an `mcp-server` subprocess over the UDS.
#[derive(Debug, Deserialize)]
pub struct BridgeRequest {
    /// Which pane is sending this request.
    pub pane_id: u64,
    /// The raw JSON-RPC 2.0 request string.
    pub request: String,
}

/// A single line written back to the subprocess.
#[derive(Debug, Serialize)]
pub struct BridgeResponse {
    /// The raw JSON-RPC 2.0 response string.
    pub response: String,
}

// ── McpBridge ─────────────────────────────────────────────────────────────────

/// Listens on a Unix domain socket and bridges MCP requests from pane
/// subprocesses into the shared `InterSessionState`.
pub struct McpBridge {
    /// Path of the socket file. Removed on `shutdown()`.
    socket_path: PathBuf,
    /// Handle to the background listener task.
    _listener_task: JoinHandle<()>,
}

impl McpBridge {
    /// Start the bridge listener on `/tmp/potato-{pid}.sock`.
    ///
    /// Returns `(McpBridge, socket_path)` — pass the path to pane env vars
    /// via `POTATO_SOCKET`.
    pub fn start(
        state: Arc<Mutex<InterSessionState>>,
        inject_tx: UnboundedSender<InjectRequest>,
    ) -> Result<(Self, PathBuf)> {
        let pid = std::process::id();
        let socket_path = PathBuf::from(format!("/tmp/potato-{pid}.sock"));
        Self::start_at(state, socket_path, Some(inject_tx))
    }

    /// Start the bridge listener at the given socket path.
    ///
    /// `inject_tx` is optional for backward-compatible tests; pass `None`
    /// to disable push delivery (messages just enqueue).
    pub fn start_at(
        state: Arc<Mutex<InterSessionState>>,
        socket_path: PathBuf,
        inject_tx: Option<UnboundedSender<InjectRequest>>,
    ) -> Result<(Self, PathBuf)> {
        // Remove a stale socket file from a previous crash.
        let _ = std::fs::remove_file(&socket_path);

        let listener = UnixListener::bind(&socket_path)
            .with_context(|| format!("failed to bind UDS at {}", socket_path.display()))?;

        let returned_path = socket_path.clone();
        let path_for_task = socket_path.clone();

        let task = tokio::spawn(async move {
            run_listener(listener, state, inject_tx, path_for_task).await;
        });
        Ok((
            Self {
                socket_path,
                _listener_task: task,
            },
            returned_path,
        ))
    }

    /// Remove the socket file. The background task will naturally stop when it
    /// can no longer accept connections.
    pub fn shutdown(&self) {
        let _ = std::fs::remove_file(&self.socket_path);
    }

    /// Returns the socket path used by this bridge.
    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }
}

impl Drop for McpBridge {
    fn drop(&mut self) {
        self.shutdown();
    }
}

// ── Listener loop ─────────────────────────────────────────────────────────────

async fn run_listener(
    listener: UnixListener,
    state: Arc<Mutex<InterSessionState>>,
    inject_tx: Option<UnboundedSender<InjectRequest>>,
    socket_path: PathBuf,
) {
    loop {
        match listener.accept().await {
            Ok((stream, _addr)) => {
                let state_clone = Arc::clone(&state);
                let tx_clone = inject_tx.clone();
                tokio::spawn(async move {
                    handle_connection(stream, state_clone, tx_clone).await;
                });
            }
            Err(e) => {
                // Log and stop. This happens when the socket is removed on shutdown.
                tracing::debug!("McpBridge accept error (likely shutdown): {e}");
                break;
            }
        }
    }

    // Best-effort cleanup.
    let _ = std::fs::remove_file(&socket_path);
}

async fn handle_connection(
    stream: tokio::net::UnixStream,
    state: Arc<Mutex<InterSessionState>>,
    inject_tx: Option<UnboundedSender<InjectRequest>>,
) {
    let (read_half, mut write_half) = stream.into_split();
    let mut reader = BufReader::new(read_half);
    let mut line = String::new();

    loop {
        line.clear();
        match reader.read_line(&mut line).await {
            Ok(0) => break, // EOF — client disconnected.
            Err(e) => {
                tracing::warn!("McpBridge read error: {e}");
                break;
            }
            Ok(_) => {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }

                let response_json = dispatch_request(trimmed, &state, &inject_tx);
                let mut response_line = response_json;
                response_line.push('\n');

                if let Err(e) = write_half.write_all(response_line.as_bytes()).await {
                    tracing::warn!("McpBridge write error: {e}");
                    break;
                }
            }
        }
    }
}

/// Parse a `BridgeRequest`, dispatch through `McpServer`, and return a
/// serialised `BridgeResponse` line (no trailing newline).
fn dispatch_request(
    line: &str,
    state: &Arc<Mutex<InterSessionState>>,
    inject_tx: &Option<UnboundedSender<InjectRequest>>,
) -> String {
    let bridge_req: BridgeRequest = match serde_json::from_str(line) {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!("McpBridge: invalid request line: {e}");
            let resp = BridgeResponse {
                response: format!(
                    r#"{{"jsonrpc":"2.0","id":null,"error":{{"code":-32700,"message":"Parse error: {e}"}}}}"#
                ),
            };
            return serde_json::to_string(&resp).unwrap_or_default();
        }
    };

    // Check if this is a send_message call — we'll need to fire injection after.
    let is_send_message = is_send_message_call(&bridge_req.request);

    let server = McpServer::new(bridge_req.pane_id, Arc::clone(state));
    let rpc_response = server.handle_request(&bridge_req.request);

    // After a successful send_message, push an injection request so the
    // main event loop writes the message into the target pane's PTY.
    if is_send_message {
        if let Some(tx) = inject_tx {
            if let Some(inject) = build_inject_request(bridge_req.pane_id, &bridge_req.request, state) {
                if let Err(e) = tx.send(inject) {
                    tracing::warn!("Failed to send inject request: {e}");
                }
            }
        }
    }

    let resp = BridgeResponse {
        response: rpc_response,
    };
    serde_json::to_string(&resp).unwrap_or_default()
}

/// Check whether a JSON-RPC request is a `tools/call` for `potato_send_message`.
fn is_send_message_call(rpc_request: &str) -> bool {
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(rpc_request) {
        v.get("method").and_then(|m| m.as_str()) == Some("tools/call")
            && v.pointer("/params/name").and_then(|n| n.as_str()) == Some("potato_send_message")
    } else {
        false
    }
}

/// Extract injection details from a send_message RPC request.
fn build_inject_request(
    from_pane: u64,
    rpc_request: &str,
    state: &Arc<Mutex<InterSessionState>>,
) -> Option<InjectRequest> {
    let v: serde_json::Value = serde_json::from_str(rpc_request).ok()?;
    let args = v.pointer("/params/arguments")?;
    let message = args.get("message")?.as_str()?;

    let st = state.lock().ok()?;
    let to_pane: u64 = match args.get("to").and_then(|t| t.as_str()) {
        Some("partner") | None => st.resolve_partner(from_pane)?,
        Some(id_str) => id_str.parse().ok()?,
    };

    let from_role = st.roles.get(&from_pane).map(|r| r.name.clone());
    drop(st);

    Some(InjectRequest {
        from_pane,
        from_role,
        to_pane,
        content: message.to_string(),
    })
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::UnixStream;

    /// Generate a unique socket path for a test.
    fn test_socket(name: &str) -> PathBuf {
        PathBuf::from(format!(
            "/tmp/potato-bridge-test-{}-{}.sock",
            name,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.subsec_nanos())
                .unwrap_or(0)
        ))
    }

    fn fresh_state() -> Arc<Mutex<InterSessionState>> {
        Arc::new(Mutex::new(InterSessionState::new()))
    }

    // ── dispatch_request unit tests (no I/O) ──────────────────────────────────

    #[test]
    fn dispatch_invalid_json_returns_parse_error() {
        let state = fresh_state();
        let result = dispatch_request("not json", &state, &None);
        let v: Value = serde_json::from_str(&result).expect("must be valid JSON");
        // BridgeResponse wraps the response string
        let inner: Value = serde_json::from_str(v["response"].as_str().unwrap()).unwrap();
        assert!(inner["error"].is_object());
    }

    #[test]
    fn dispatch_initialize_returns_protocol_version() {
        let state = fresh_state();
        let req = json!({
            "pane_id": 0,
            "request": r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"test","version":"1"}}}"#
        });
        let result = dispatch_request(&req.to_string(), &state, &None);
        let v: Value = serde_json::from_str(&result).expect("valid JSON");
        let inner: Value = serde_json::from_str(v["response"].as_str().unwrap()).unwrap();
        assert_eq!(
            inner["result"]["protocolVersion"],
            crate::mcp::server::PROTOCOL_VERSION
        );
    }

    #[test]
    fn dispatch_tools_list_returns_tools() {
        let state = fresh_state();
        let req = json!({
            "pane_id": 0,
            "request": r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#
        });
        let result = dispatch_request(&req.to_string(), &state, &None);
        let v: Value = serde_json::from_str(&result).expect("valid JSON");
        let inner: Value = serde_json::from_str(v["response"].as_str().unwrap()).unwrap();
        assert!(inner["result"]["tools"].is_array());
        assert!(!inner["result"]["tools"].as_array().unwrap().is_empty());
    }

    #[test]
    fn dispatch_unknown_method_returns_method_not_found() {
        let state = fresh_state();
        let req = json!({
            "pane_id": 0,
            "request": r#"{"jsonrpc":"2.0","id":3,"method":"unknown/method"}"#
        });
        let result = dispatch_request(&req.to_string(), &state, &None);
        let v: Value = serde_json::from_str(&result).expect("valid JSON");
        let inner: Value = serde_json::from_str(v["response"].as_str().unwrap()).unwrap();
        assert_eq!(inner["error"]["code"], crate::mcp::protocol::METHOD_NOT_FOUND);
    }

    // ── Integration tests: real UDS round-trip ────────────────────────────────

    #[tokio::test]
    async fn bridge_start_and_connect() {
        let state = fresh_state();
        let sock = test_socket("start");
        let (_bridge, path) = McpBridge::start_at(state, sock, None).unwrap();

        // Connect and immediately disconnect.
        let stream = UnixStream::connect(&path).await.expect("connect failed");
        drop(stream);
    }

    #[tokio::test]
    async fn bridge_initialize_roundtrip() {
        let state = fresh_state();
        let sock = test_socket("init-rt");
        let (_bridge, path) = McpBridge::start_at(state, sock, None).unwrap();

        let stream = UnixStream::connect(&path).await.expect("connect");
        let (read_half, mut write_half) = stream.into_split();
        let mut reader = BufReader::new(read_half);

        let req = json!({
            "pane_id": 0,
            "request": r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"test","version":"1"}}}"#
        });
        let mut line_to_send = req.to_string();
        line_to_send.push('\n');
        write_half.write_all(line_to_send.as_bytes()).await.unwrap();

        let mut response_line = String::new();
        reader.read_line(&mut response_line).await.unwrap();

        let v: Value = serde_json::from_str(response_line.trim()).expect("valid JSON");
        let inner: Value = serde_json::from_str(v["response"].as_str().unwrap()).unwrap();
        assert_eq!(
            inner["result"]["protocolVersion"],
            crate::mcp::server::PROTOCOL_VERSION
        );
    }

    #[tokio::test]
    async fn bridge_multiple_requests_same_connection() {
        let state = fresh_state();
        let sock = test_socket("multi-req");
        let (_bridge, path) = McpBridge::start_at(state, sock, None).unwrap();

        let stream = UnixStream::connect(&path).await.expect("connect");
        let (read_half, mut write_half) = stream.into_split();
        let mut reader = BufReader::new(read_half);

        // Send initialize
        let init_req = json!({
            "pane_id": 0,
            "request": r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"t","version":"1"}}}"#
        });
        let mut msg = init_req.to_string();
        msg.push('\n');
        write_half.write_all(msg.as_bytes()).await.unwrap();

        let mut resp = String::new();
        reader.read_line(&mut resp).await.unwrap();
        let v: Value = serde_json::from_str(resp.trim()).unwrap();
        assert!(v["response"].is_string());

        // Send tools/list on the same connection
        let list_req = json!({
            "pane_id": 0,
            "request": r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#
        });
        let mut msg2 = list_req.to_string();
        msg2.push('\n');
        write_half.write_all(msg2.as_bytes()).await.unwrap();

        let mut resp2 = String::new();
        reader.read_line(&mut resp2).await.unwrap();
        let v2: Value = serde_json::from_str(resp2.trim()).unwrap();
        let inner2: Value = serde_json::from_str(v2["response"].as_str().unwrap()).unwrap();
        assert!(inner2["result"]["tools"].is_array());
    }

    #[tokio::test]
    async fn bridge_simultaneous_connections() {
        let state = fresh_state();
        let sock = test_socket("simultaneous");
        let (_bridge, path) = McpBridge::start_at(Arc::clone(&state), sock, None).unwrap();

        // Two concurrent connections from pane 0 and pane 1.
        let path0 = path.clone();
        let path1 = path.clone();

        let h0 = tokio::spawn(async move {
            let stream = UnixStream::connect(&path0).await.unwrap();
            let (r, mut w) = stream.into_split();
            let mut reader = BufReader::new(r);
            let req = json!({
                "pane_id": 0,
                "request": r#"{"jsonrpc":"2.0","id":10,"method":"tools/list"}"#
            });
            let mut line = req.to_string();
            line.push('\n');
            w.write_all(line.as_bytes()).await.unwrap();
            let mut resp = String::new();
            reader.read_line(&mut resp).await.unwrap();
            resp
        });

        let h1 = tokio::spawn(async move {
            let stream = UnixStream::connect(&path1).await.unwrap();
            let (r, mut w) = stream.into_split();
            let mut reader = BufReader::new(r);
            let req = json!({
                "pane_id": 1,
                "request": r#"{"jsonrpc":"2.0","id":11,"method":"tools/list"}"#
            });
            let mut line = req.to_string();
            line.push('\n');
            w.write_all(line.as_bytes()).await.unwrap();
            let mut resp = String::new();
            reader.read_line(&mut resp).await.unwrap();
            resp
        });

        let (r0, r1) = tokio::join!(h0, h1);
        for raw in [r0.unwrap(), r1.unwrap()] {
            let v: Value = serde_json::from_str(raw.trim()).unwrap();
            let inner: Value = serde_json::from_str(v["response"].as_str().unwrap()).unwrap();
            assert!(inner["result"]["tools"].is_array());
        }
    }

    #[tokio::test]
    async fn bridge_shutdown_removes_socket() {
        let state = fresh_state();
        let sock = test_socket("shutdown");
        let (bridge, path) = McpBridge::start_at(state, sock, None).unwrap();
        assert!(path.exists());
        bridge.shutdown();
        // Give the OS a moment to process.
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        assert!(!path.exists());
    }

    #[tokio::test]
    async fn bridge_drop_removes_socket() {
        let state = fresh_state();
        let sock = test_socket("drop");
        let (bridge, path) = McpBridge::start_at(state, sock, None).unwrap();
        assert!(path.exists());
        drop(bridge);
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        assert!(!path.exists());
    }

    #[tokio::test]
    async fn bridge_start_at_overwrites_stale_socket() {
        let sock = test_socket("stale-sock");
        // Create a stale file at the socket path.
        std::fs::write(&sock, b"stale").unwrap();
        assert!(sock.exists());

        let state = fresh_state();
        // Should succeed even though the file exists.
        let (_bridge, path) = McpBridge::start_at(state, sock, None).unwrap();
        // Should be connectable.
        UnixStream::connect(&path).await.expect("connect after stale removal");
    }

    #[tokio::test]
    async fn bridge_cross_pane_messaging() {
        let state = fresh_state();
        let sock = test_socket("cross-pane");
        let (_bridge, path) = McpBridge::start_at(Arc::clone(&state), sock, None).unwrap();

        // Pane 0 sends a message to pane 1.
        let stream0 = UnixStream::connect(&path).await.unwrap();
        let (r0, mut w0) = stream0.into_split();
        let mut reader0 = BufReader::new(r0);

        let send_req = json!({
            "pane_id": 0,
            "request": r#"{"jsonrpc":"2.0","id":20,"method":"tools/call","params":{"name":"potato_send_message","arguments":{"message":"hello pane1","to":"1"}}}"#
        });
        let mut line = send_req.to_string();
        line.push('\n');
        w0.write_all(line.as_bytes()).await.unwrap();
        let mut resp0 = String::new();
        reader0.read_line(&mut resp0).await.unwrap();
        let v0: Value = serde_json::from_str(resp0.trim()).unwrap();
        let inner0: Value = serde_json::from_str(v0["response"].as_str().unwrap()).unwrap();
        assert!(inner0["error"].is_null(), "send failed: {inner0}");

        // Pane 1 reads messages.
        let stream1 = UnixStream::connect(&path).await.unwrap();
        let (r1, mut w1) = stream1.into_split();
        let mut reader1 = BufReader::new(r1);

        let get_req = json!({
            "pane_id": 1,
            "request": r#"{"jsonrpc":"2.0","id":21,"method":"tools/call","params":{"name":"potato_get_messages","arguments":{}}}"#
        });
        let mut line1 = get_req.to_string();
        line1.push('\n');
        w1.write_all(line1.as_bytes()).await.unwrap();
        let mut resp1 = String::new();
        reader1.read_line(&mut resp1).await.unwrap();
        let v1: Value = serde_json::from_str(resp1.trim()).unwrap();
        let inner1: Value = serde_json::from_str(v1["response"].as_str().unwrap()).unwrap();
        assert!(inner1["error"].is_null());
        let text = inner1["result"]["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("hello pane1"), "expected message, got: {text}");
    }
}
