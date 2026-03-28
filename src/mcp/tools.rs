//! MCP tool definitions and dispatch for the Potato inter-session server.
//!
//! `TOOL_DEFINITIONS` enumerates all 8 tools (as a `Vec` built once at call time).
//! `handle_tool_call` dispatches a `tools/call` request to the correct state method.

use std::sync::{Arc, Mutex};

use serde_json::{json, Value};

use crate::mcp::protocol::{CallToolResult, ToolInfo};
use crate::mcp::state::{ClaimResult, InterSessionState, MessagePriority, PaneRole, RoleClaimResult};

// ── Tool names ────────────────────────────────────────────────────────────────

pub const TOOL_SEND_MESSAGE: &str = "potato_send_message";
pub const TOOL_GET_MESSAGES: &str = "potato_get_messages";
pub const TOOL_GET_PARTNER_STATUS: &str = "potato_get_partner_status";
pub const TOOL_SHARED_CONTEXT: &str = "potato_shared_context";
pub const TOOL_CLAIM_TASK: &str = "potato_claim_task";
pub const TOOL_RELEASE_TASK: &str = "potato_release_task";
pub const TOOL_CLAIM_ROLE: &str = "potato_claim_role";
pub const TOOL_GET_ROLE: &str = "potato_get_role";

// ── Tool definitions ──────────────────────────────────────────────────────────

/// Return all 6 Potato MCP tool definitions with full JSON schemas.
pub fn tool_definitions() -> Vec<ToolInfo> {
    vec![
        ToolInfo {
            name: TOOL_SEND_MESSAGE.into(),
            description: "Send a message to another agent session running in Potato. \
                The message will be delivered to the target pane's agent.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "to": {
                        "type": "string",
                        "description": "Target pane identifier. Use 'partner' for the other pane, or a specific pane ID as a number string."
                    },
                    "message": {
                        "type": "string",
                        "description": "The message content to send."
                    },
                    "priority": {
                        "type": "string",
                        "enum": ["normal", "urgent"],
                        "default": "normal",
                        "description": "Urgent messages trigger immediate PTY injection."
                    }
                },
                "required": ["message"]
            }),
        },
        ToolInfo {
            name: TOOL_GET_MESSAGES.into(),
            description: "Check for messages from other agent sessions. Returns any unread messages.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "mark_read": {
                        "type": "boolean",
                        "default": true,
                        "description": "Whether to mark returned messages as read."
                    }
                }
            }),
        },
        ToolInfo {
            name: TOOL_GET_PARTNER_STATUS.into(),
            description: "Get the current status of the other agent session(s) in Potato.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {}
            }),
        },
        ToolInfo {
            name: TOOL_SHARED_CONTEXT.into(),
            description: "Read or write shared context that all agent sessions can access. \
                Use for coordination, shared state, and working agreements.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "op": {
                        "type": "string",
                        "enum": ["get", "set", "delete", "list"],
                        "description": "Operation to perform."
                    },
                    "key": {
                        "type": "string",
                        "description": "Context key (required for get/set/delete)."
                    },
                    "value": {
                        "description": "Value to store (required for set). Can be any JSON value."
                    }
                },
                "required": ["op"]
            }),
        },
        ToolInfo {
            name: TOOL_CLAIM_TASK.into(),
            description: "Claim a task so other sessions know you're working on it. Prevents duplicate work.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "task_id": {
                        "type": "string",
                        "description": "Unique task identifier."
                    },
                    "description": {
                        "type": "string",
                        "description": "Human-readable description of the task."
                    }
                },
                "required": ["task_id"]
            }),
        },
        ToolInfo {
            name: TOOL_RELEASE_TASK.into(),
            description: "Release a task you previously claimed.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "task_id": {
                        "type": "string",
                        "description": "Task identifier to release."
                    }
                },
                "required": ["task_id"]
            }),
        },
        ToolInfo {
            name: TOOL_CLAIM_ROLE.into(),
            description: "Claim a role for this session. If another agent already holds this role \
                name, the claim is rejected — pick a different role. Use potato_get_role first to \
                see which roles are already taken.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "role": {
                        "type": "string",
                        "description": "The role name to claim (e.g. 'architect', 'implementer', 'reviewer')."
                    },
                    "description": {
                        "type": "string",
                        "description": "A short description of what this role does in the current collaboration."
                    }
                },
                "required": ["role"]
            }),
        },
        ToolInfo {
            name: TOOL_GET_ROLE.into(),
            description: "Get this session's assigned role and see all roles currently claimed across panes. \
                Check this before claiming a role to avoid conflicts.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {}
            }),
        },
    ]
}

// ── Dispatch ──────────────────────────────────────────────────────────────────

/// Dispatch a tool call to the appropriate state handler.
///
/// `pane_id` identifies which pane is making the call.
pub fn handle_tool_call(
    name: &str,
    args: &Value,
    pane_id: u64,
    state: &Arc<Mutex<InterSessionState>>,
) -> CallToolResult {
    match name {
        TOOL_SEND_MESSAGE => handle_send_message(args, pane_id, state),
        TOOL_GET_MESSAGES => handle_get_messages(args, pane_id, state),
        TOOL_GET_PARTNER_STATUS => handle_get_partner_status(pane_id, state),
        TOOL_SHARED_CONTEXT => handle_shared_context(args, state),
        TOOL_CLAIM_TASK => handle_claim_task(args, pane_id, state),
        TOOL_RELEASE_TASK => handle_release_task(args, pane_id, state),
        TOOL_CLAIM_ROLE => handle_claim_role(args, pane_id, state),
        TOOL_GET_ROLE => handle_get_role(pane_id, state),
        unknown => CallToolResult::failure(format!("Unknown tool: {unknown}")),
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Lock the shared state, returning a `CallToolResult::failure` on poison.
macro_rules! lock_state {
    ($state:expr) => {
        match $state.lock() {
            Ok(g) => g,
            Err(_) => return CallToolResult::failure("State lock poisoned"),
        }
    };
}

/// Build the `all_roles` JSON array from the current state.
fn collect_all_roles(st: &InterSessionState, self_pane: Option<u64>) -> Vec<Value> {
    st.list_roles()
        .iter()
        .map(|(id, r)| {
            let mut obj = json!({"pane_id": id, "role": r.name, "description": r.description});
            if let Some(self_id) = self_pane {
                obj["is_self"] = json!(*id == self_id);
            }
            obj
        })
        .collect()
}

// ── Individual handlers ───────────────────────────────────────────────────────

fn handle_send_message(
    args: &Value,
    pane_id: u64,
    state: &Arc<Mutex<InterSessionState>>,
) -> CallToolResult {
    let message = match args.get("message").and_then(Value::as_str) {
        Some(m) => m.to_string(),
        None => return CallToolResult::failure("Missing required field: message"),
    };

    let priority = match args.get("priority").and_then(Value::as_str) {
        Some("urgent") => MessagePriority::Urgent,
        Some("normal") | None => MessagePriority::Normal,
        Some(other) => {
            return CallToolResult::failure(format!("Invalid priority: {other}. Must be 'normal' or 'urgent'"));
        }
    };

    // Resolve target pane.
    let to_pane: u64 = match args.get("to").and_then(Value::as_str) {
        Some("partner") | None => {
            // Send to all other known panes (determined by roles or inboxes).
            // For now, resolve to pane_id XOR 1 (0↔1 in a 2-pane setup).
            // This is the simplest useful default for 2-pane scenarios.
            pane_id ^ 1
        }
        Some(id_str) => match id_str.parse::<u64>() {
            Ok(id) => id,
            Err(_) => return CallToolResult::failure(format!("Invalid target pane id: {id_str}")),
        },
    };

    let mut st = lock_state!(state);
    st.send_message(pane_id, to_pane, &message, priority);

    CallToolResult::success(format!(
        "Message delivered to pane {to_pane}. Priority: {}.",
        match priority {
            MessagePriority::Normal => "normal",
            MessagePriority::Urgent => "urgent",
        }
    ))
}

fn handle_get_messages(
    args: &Value,
    pane_id: u64,
    state: &Arc<Mutex<InterSessionState>>,
) -> CallToolResult {
    let mark_read = args
        .get("mark_read")
        .and_then(Value::as_bool)
        .unwrap_or(true);

    let mut st = lock_state!(state);
    let messages = st.get_messages(pane_id, mark_read);

    if messages.is_empty() {
        return CallToolResult::success("No unread messages.");
    }

    let msg_json: Vec<Value> = messages
        .iter()
        .map(|m| {
            json!({
                "from_pane": m.from_pane,
                "content": m.content,
                "priority": m.priority,
                "timestamp": m.timestamp.to_rfc3339(),
                "read": m.read
            })
        })
        .collect();

    CallToolResult::success(serde_json::to_string_pretty(&msg_json).unwrap_or_default())
}

fn handle_get_partner_status(
    pane_id: u64,
    state: &Arc<Mutex<InterSessionState>>,
) -> CallToolResult {
    let st = lock_state!(state);
    let partners = st.get_partner_status(pane_id);

    if partners.is_empty() {
        return CallToolResult::success("No partner panes found.");
    }

    let panes: Vec<Value> = partners
        .iter()
        .map(|p| {
            json!({
                "pane_id": p.pane_id,
                "role": p.role.name,
                "role_description": p.role.description,
                "unread_messages": p.unread_messages
            })
        })
        .collect();

    let result = json!({ "panes": panes });
    CallToolResult::success(serde_json::to_string_pretty(&result).unwrap_or_default())
}

fn handle_shared_context(
    args: &Value,
    state: &Arc<Mutex<InterSessionState>>,
) -> CallToolResult {
    let op = match args.get("op").and_then(Value::as_str) {
        Some(o) => o,
        None => return CallToolResult::failure("Missing required field: op"),
    };

    match op {
        "get" => {
            let key = match args.get("key").and_then(Value::as_str) {
                Some(k) => k,
                None => return CallToolResult::failure("Missing required field: key (for op=get)"),
            };
            let st = match state.lock() {
                Ok(g) => g,
                Err(_) => return CallToolResult::failure("State lock poisoned"),
            };
            match st.shared_context_get(key) {
                Some(val) => CallToolResult::success(val.to_string()),
                None => CallToolResult::success(format!("Key '{key}' not found.")),
            }
        }
        "set" => {
            let key = match args.get("key").and_then(Value::as_str) {
                Some(k) => k,
                None => return CallToolResult::failure("Missing required field: key (for op=set)"),
            };
            let value = match args.get("value") {
                Some(v) => v.clone(),
                None => return CallToolResult::failure("Missing required field: value (for op=set)"),
            };
            let mut st = match state.lock() {
                Ok(g) => g,
                Err(_) => return CallToolResult::failure("State lock poisoned"),
            };
            st.shared_context_set(key, value);
            CallToolResult::success(format!("Set '{key}'."))
        }
        "delete" => {
            let key = match args.get("key").and_then(Value::as_str) {
                Some(k) => k,
                None => return CallToolResult::failure("Missing required field: key (for op=delete)"),
            };
            let mut st = match state.lock() {
                Ok(g) => g,
                Err(_) => return CallToolResult::failure("State lock poisoned"),
            };
            if st.shared_context_delete(key) {
                CallToolResult::success(format!("Deleted '{key}'."))
            } else {
                CallToolResult::success(format!("Key '{key}' not found."))
            }
        }
        "list" => {
            let st = match state.lock() {
                Ok(g) => g,
                Err(_) => return CallToolResult::failure("State lock poisoned"),
            };
            let keys = st.shared_context_list();
            if keys.is_empty() {
                CallToolResult::success("No keys in shared context.")
            } else {
                CallToolResult::success(keys.join("\n"))
            }
        }
        other => CallToolResult::failure(format!(
            "Unknown op: {other}. Must be one of: get, set, delete, list"
        )),
    }
}

fn handle_claim_task(
    args: &Value,
    pane_id: u64,
    state: &Arc<Mutex<InterSessionState>>,
) -> CallToolResult {
    let task_id = match args.get("task_id").and_then(Value::as_str) {
        Some(id) => id.to_string(),
        None => return CallToolResult::failure("Missing required field: task_id"),
    };
    let description = args
        .get("description")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();

    let mut st = lock_state!(state);
    match st.claim_task(&task_id, &description, pane_id) {
        ClaimResult::Claimed => {
            CallToolResult::success(serde_json::to_string(&json!({"claimed": true})).unwrap())
        }
        ClaimResult::AlreadyClaimed { held_by, since } => CallToolResult::success(
            serde_json::to_string(&json!({
                "claimed": false,
                "held_by": format!("pane-{held_by}"),
                "since": since.to_rfc3339()
            }))
            .unwrap(),
        ),
    }
}

fn handle_release_task(
    args: &Value,
    pane_id: u64,
    state: &Arc<Mutex<InterSessionState>>,
) -> CallToolResult {
    let task_id = match args.get("task_id").and_then(Value::as_str) {
        Some(id) => id,
        None => return CallToolResult::failure("Missing required field: task_id"),
    };

    let mut st = lock_state!(state);
    if st.release_task(task_id, pane_id) {
        CallToolResult::success(format!("Released task '{task_id}'."))
    } else {
        CallToolResult::failure(format!(
            "Cannot release '{task_id}': task not found or not owned by this pane."
        ))
    }
}

fn handle_claim_role(
    args: &Value,
    pane_id: u64,
    state: &Arc<Mutex<InterSessionState>>,
) -> CallToolResult {
    let role_name = match args.get("role").and_then(Value::as_str) {
        Some(r) => r.to_string(),
        None => return CallToolResult::failure("Missing required field: role"),
    };
    let description = args
        .get("description")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();

    let mut st = lock_state!(state);

    let role = PaneRole { name: role_name.clone(), description };
    match st.claim_role(pane_id, role) {
        RoleClaimResult::Claimed => {
            let all_roles = collect_all_roles(&st, None);
            CallToolResult::success(serde_json::to_string_pretty(&json!({
                "claimed": true,
                "role": role_name,
                "all_roles": all_roles
            })).unwrap_or_default())
        }
        RoleClaimResult::AlreadyClaimed { held_by } => {
            let all_roles = collect_all_roles(&st, None);
            CallToolResult::success(serde_json::to_string_pretty(&json!({
                "claimed": false,
                "role": role_name,
                "held_by": format!("pane-{held_by}"),
                "reason": format!("Role '{}' is already claimed by pane {}. Pick a different role.", role_name, held_by),
                "all_roles": all_roles
            })).unwrap_or_default())
        }
    }
}

fn handle_get_role(
    pane_id: u64,
    state: &Arc<Mutex<InterSessionState>>,
) -> CallToolResult {
    let st = lock_state!(state);
    let role = st.get_role(pane_id);

    let all_roles = collect_all_roles(&st, Some(pane_id));

    let result = json!({
        "pane_id": pane_id,
        "your_role": role.map(|r| r.name.clone()).unwrap_or_else(|| "unassigned".to_string()),
        "your_role_description": role.map(|r| r.description.clone()).unwrap_or_default(),
        "all_roles": all_roles
    });

    CallToolResult::success(serde_json::to_string_pretty(&result).unwrap_or_default())
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn make_state() -> Arc<Mutex<InterSessionState>> {
        Arc::new(Mutex::new(InterSessionState::new()))
    }

    fn make_state_with_roles() -> Arc<Mutex<InterSessionState>> {
        let state = make_state();
        {
            let mut st = state.lock().unwrap();
            st.set_role(0, PaneRole { name: "architect".into(), description: "Designs systems".into() });
            st.set_role(1, PaneRole { name: "implementer".into(), description: "Builds things".into() });
        }
        state
    }

    // ── tool_definitions ─────────────────────────────────────────────────────

    #[test]
    fn tool_definitions_returns_all_tools() {
        let tools = tool_definitions();
        assert_eq!(tools.len(), 8); // 6 spec tools + claim_role + get_role
    }

    #[test]
    fn tool_definitions_have_unique_names() {
        let tools = tool_definitions();
        let mut names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), tools.len());
    }

    #[test]
    fn tool_definitions_include_all_known_tools() {
        let tools = tool_definitions();
        let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
        assert!(names.contains(&TOOL_SEND_MESSAGE));
        assert!(names.contains(&TOOL_GET_MESSAGES));
        assert!(names.contains(&TOOL_GET_PARTNER_STATUS));
        assert!(names.contains(&TOOL_SHARED_CONTEXT));
        assert!(names.contains(&TOOL_CLAIM_TASK));
        assert!(names.contains(&TOOL_RELEASE_TASK));
        assert!(names.contains(&TOOL_CLAIM_ROLE));
        assert!(names.contains(&TOOL_GET_ROLE));
    }

    #[test]
    fn all_tools_have_non_empty_descriptions() {
        for tool in tool_definitions() {
            assert!(!tool.description.is_empty(), "Tool {} has empty description", tool.name);
        }
    }

    #[test]
    fn all_tools_have_object_input_schema() {
        for tool in tool_definitions() {
            assert_eq!(
                tool.input_schema["type"],
                "object",
                "Tool {} input_schema.type != 'object'",
                tool.name
            );
        }
    }

    // ── Unknown tool ──────────────────────────────────────────────────────────

    #[test]
    fn unknown_tool_returns_error() {
        let state = make_state();
        let result = handle_tool_call("potato_does_not_exist", &json!({}), 0, &state);
        assert!(result.is_error);
        assert!(result.content[0].text.contains("Unknown tool"));
    }

    // ── potato_send_message ───────────────────────────────────────────────────

    #[test]
    fn send_message_basic() {
        let state = make_state();
        let args = json!({"message": "hello partner"});
        let result = handle_tool_call(TOOL_SEND_MESSAGE, &args, 0, &state);
        assert!(!result.is_error);
        // Verify message arrived in inbox of pane 1.
        let msgs = state.lock().unwrap().get_messages(1, false);
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].content, "hello partner");
    }

    #[test]
    fn send_message_to_specific_pane() {
        let state = make_state();
        let args = json!({"message": "direct", "to": "2"});
        let result = handle_tool_call(TOOL_SEND_MESSAGE, &args, 0, &state);
        assert!(!result.is_error);
        let msgs = state.lock().unwrap().get_messages(2, false);
        assert_eq!(msgs.len(), 1);
    }

    #[test]
    fn send_message_urgent_priority() {
        let state = make_state();
        let args = json!({"message": "urgent!", "priority": "urgent"});
        let result = handle_tool_call(TOOL_SEND_MESSAGE, &args, 0, &state);
        assert!(!result.is_error);
        let msgs = state.lock().unwrap().get_messages(1, false);
        assert_eq!(msgs[0].priority, MessagePriority::Urgent);
    }

    #[test]
    fn send_message_missing_message_field() {
        let state = make_state();
        let result = handle_tool_call(TOOL_SEND_MESSAGE, &json!({"to": "1"}), 0, &state);
        assert!(result.is_error);
        assert!(result.content[0].text.contains("Missing required field: message"));
    }

    #[test]
    fn send_message_invalid_priority() {
        let state = make_state();
        let args = json!({"message": "hi", "priority": "super_urgent"});
        let result = handle_tool_call(TOOL_SEND_MESSAGE, &args, 0, &state);
        assert!(result.is_error);
        assert!(result.content[0].text.contains("Invalid priority"));
    }

    #[test]
    fn send_message_invalid_pane_id() {
        let state = make_state();
        let args = json!({"message": "hi", "to": "not_a_number"});
        let result = handle_tool_call(TOOL_SEND_MESSAGE, &args, 0, &state);
        assert!(result.is_error);
    }

    // ── potato_get_messages ───────────────────────────────────────────────────

    #[test]
    fn get_messages_empty_inbox() {
        let state = make_state();
        let result = handle_tool_call(TOOL_GET_MESSAGES, &json!({}), 0, &state);
        assert!(!result.is_error);
        assert!(result.content[0].text.contains("No unread messages"));
    }

    #[test]
    fn get_messages_returns_messages() {
        let state = make_state();
        state.lock().unwrap().send_message(1, 0, "from pane 1", MessagePriority::Normal);
        let result = handle_tool_call(TOOL_GET_MESSAGES, &json!({}), 0, &state);
        assert!(!result.is_error);
        assert!(result.content[0].text.contains("from pane 1"));
    }

    #[test]
    fn get_messages_marks_read_by_default() {
        let state = make_state();
        state.lock().unwrap().send_message(1, 0, "msg", MessagePriority::Normal);
        handle_tool_call(TOOL_GET_MESSAGES, &json!({}), 0, &state);
        // Second call should return empty.
        let result2 = handle_tool_call(TOOL_GET_MESSAGES, &json!({}), 0, &state);
        assert!(result2.content[0].text.contains("No unread messages"));
    }

    #[test]
    fn get_messages_does_not_mark_read_when_false() {
        let state = make_state();
        state.lock().unwrap().send_message(1, 0, "msg", MessagePriority::Normal);
        handle_tool_call(TOOL_GET_MESSAGES, &json!({"mark_read": false}), 0, &state);
        // Should still be unread.
        let result2 = handle_tool_call(TOOL_GET_MESSAGES, &json!({"mark_read": false}), 0, &state);
        assert!(result2.content[0].text.contains("msg"));
    }

    // ── potato_get_partner_status ─────────────────────────────────────────────

    #[test]
    fn get_partner_status_no_partners() {
        let state = make_state();
        let result = handle_tool_call(TOOL_GET_PARTNER_STATUS, &json!({}), 0, &state);
        assert!(!result.is_error);
        assert!(result.content[0].text.contains("No partner panes"));
    }

    #[test]
    fn get_partner_status_returns_partners() {
        let state = make_state_with_roles();
        let result = handle_tool_call(TOOL_GET_PARTNER_STATUS, &json!({}), 0, &state);
        assert!(!result.is_error);
        assert!(result.content[0].text.contains("implementer"));
    }

    #[test]
    fn get_partner_status_excludes_self() {
        let state = make_state_with_roles();
        let result = handle_tool_call(TOOL_GET_PARTNER_STATUS, &json!({}), 0, &state);
        assert!(!result.is_error);
        // Should NOT contain own role "architect"
        assert!(!result.content[0].text.contains(r#""pane_id": 0"#));
    }

    // ── potato_shared_context ─────────────────────────────────────────────────

    #[test]
    fn shared_context_set_and_get() {
        let state = make_state();
        handle_tool_call(TOOL_SHARED_CONTEXT, &json!({"op": "set", "key": "k", "value": "v"}), 0, &state);
        let result = handle_tool_call(TOOL_SHARED_CONTEXT, &json!({"op": "get", "key": "k"}), 0, &state);
        assert!(!result.is_error);
        assert!(result.content[0].text.contains("v"));
    }

    #[test]
    fn shared_context_get_missing_key() {
        let state = make_state();
        let result = handle_tool_call(TOOL_SHARED_CONTEXT, &json!({"op": "get", "key": "nope"}), 0, &state);
        assert!(!result.is_error);
        assert!(result.content[0].text.contains("not found"));
    }

    #[test]
    fn shared_context_delete_existing() {
        let state = make_state();
        handle_tool_call(TOOL_SHARED_CONTEXT, &json!({"op": "set", "key": "k", "value": 1}), 0, &state);
        let result = handle_tool_call(TOOL_SHARED_CONTEXT, &json!({"op": "delete", "key": "k"}), 0, &state);
        assert!(!result.is_error);
        assert!(result.content[0].text.contains("Deleted"));
    }

    #[test]
    fn shared_context_delete_missing() {
        let state = make_state();
        let result = handle_tool_call(TOOL_SHARED_CONTEXT, &json!({"op": "delete", "key": "ghost"}), 0, &state);
        assert!(!result.is_error);
        assert!(result.content[0].text.contains("not found"));
    }

    #[test]
    fn shared_context_list() {
        let state = make_state();
        handle_tool_call(TOOL_SHARED_CONTEXT, &json!({"op": "set", "key": "b", "value": 1}), 0, &state);
        handle_tool_call(TOOL_SHARED_CONTEXT, &json!({"op": "set", "key": "a", "value": 2}), 0, &state);
        let result = handle_tool_call(TOOL_SHARED_CONTEXT, &json!({"op": "list"}), 0, &state);
        assert!(!result.is_error);
        assert!(result.content[0].text.contains("a"));
        assert!(result.content[0].text.contains("b"));
    }

    #[test]
    fn shared_context_list_empty() {
        let state = make_state();
        let result = handle_tool_call(TOOL_SHARED_CONTEXT, &json!({"op": "list"}), 0, &state);
        assert!(!result.is_error);
        assert!(result.content[0].text.contains("No keys"));
    }

    #[test]
    fn shared_context_missing_op() {
        let state = make_state();
        let result = handle_tool_call(TOOL_SHARED_CONTEXT, &json!({}), 0, &state);
        assert!(result.is_error);
        assert!(result.content[0].text.contains("Missing required field: op"));
    }

    #[test]
    fn shared_context_unknown_op() {
        let state = make_state();
        let result = handle_tool_call(TOOL_SHARED_CONTEXT, &json!({"op": "hack"}), 0, &state);
        assert!(result.is_error);
        assert!(result.content[0].text.contains("Unknown op"));
    }

    #[test]
    fn shared_context_set_missing_key() {
        let state = make_state();
        let result = handle_tool_call(TOOL_SHARED_CONTEXT, &json!({"op": "set", "value": 1}), 0, &state);
        assert!(result.is_error);
        assert!(result.content[0].text.contains("Missing required field: key"));
    }

    #[test]
    fn shared_context_set_missing_value() {
        let state = make_state();
        let result = handle_tool_call(TOOL_SHARED_CONTEXT, &json!({"op": "set", "key": "k"}), 0, &state);
        assert!(result.is_error);
        assert!(result.content[0].text.contains("Missing required field: value"));
    }

    // ── potato_claim_task ─────────────────────────────────────────────────────

    #[test]
    fn claim_task_success() {
        let state = make_state();
        let result = handle_tool_call(TOOL_CLAIM_TASK, &json!({"task_id": "task-1"}), 0, &state);
        assert!(!result.is_error);
        let parsed: Value = serde_json::from_str(&result.content[0].text).unwrap();
        assert_eq!(parsed["claimed"], true);
    }

    #[test]
    fn claim_task_already_claimed() {
        let state = make_state();
        handle_tool_call(TOOL_CLAIM_TASK, &json!({"task_id": "task-1"}), 0, &state);
        let result = handle_tool_call(TOOL_CLAIM_TASK, &json!({"task_id": "task-1"}), 1, &state);
        assert!(!result.is_error); // Not an error — just returns claimed:false
        let parsed: Value = serde_json::from_str(&result.content[0].text).unwrap();
        assert_eq!(parsed["claimed"], false);
        assert!(parsed["held_by"].as_str().unwrap().contains("pane-0"));
    }

    #[test]
    fn claim_task_missing_task_id() {
        let state = make_state();
        let result = handle_tool_call(TOOL_CLAIM_TASK, &json!({}), 0, &state);
        assert!(result.is_error);
        assert!(result.content[0].text.contains("Missing required field: task_id"));
    }

    // ── potato_release_task ───────────────────────────────────────────────────

    #[test]
    fn release_task_success() {
        let state = make_state();
        handle_tool_call(TOOL_CLAIM_TASK, &json!({"task_id": "task-1"}), 0, &state);
        let result = handle_tool_call(TOOL_RELEASE_TASK, &json!({"task_id": "task-1"}), 0, &state);
        assert!(!result.is_error);
        assert!(result.content[0].text.contains("Released"));
    }

    #[test]
    fn release_task_not_owner() {
        let state = make_state();
        handle_tool_call(TOOL_CLAIM_TASK, &json!({"task_id": "task-1"}), 0, &state);
        let result = handle_tool_call(TOOL_RELEASE_TASK, &json!({"task_id": "task-1"}), 1, &state);
        assert!(result.is_error);
        assert!(result.content[0].text.contains("Cannot release"));
    }

    #[test]
    fn release_task_missing_task_id() {
        let state = make_state();
        let result = handle_tool_call(TOOL_RELEASE_TASK, &json!({}), 0, &state);
        assert!(result.is_error);
        assert!(result.content[0].text.contains("Missing required field: task_id"));
    }

    // ── potato_claim_role ──────────────────────────────────────────────────────

    #[test]
    fn claim_role_success() {
        let state = make_state();
        let result = handle_tool_call(TOOL_CLAIM_ROLE, &json!({"role": "architect", "description": "Designs"}), 0, &state);
        assert!(!result.is_error);
        let parsed: Value = serde_json::from_str(&result.content[0].text).unwrap();
        assert_eq!(parsed["claimed"], true);
        assert_eq!(parsed["role"], "architect");
    }

    #[test]
    fn claim_role_rejected_when_taken() {
        let state = make_state();
        handle_tool_call(TOOL_CLAIM_ROLE, &json!({"role": "architect"}), 0, &state);
        let result = handle_tool_call(TOOL_CLAIM_ROLE, &json!({"role": "architect"}), 1, &state);
        assert!(!result.is_error);
        let parsed: Value = serde_json::from_str(&result.content[0].text).unwrap();
        assert_eq!(parsed["claimed"], false);
        assert!(parsed["held_by"].as_str().unwrap().contains("pane-0"));
    }

    #[test]
    fn claim_role_case_insensitive() {
        let state = make_state();
        handle_tool_call(TOOL_CLAIM_ROLE, &json!({"role": "Architect"}), 0, &state);
        let result = handle_tool_call(TOOL_CLAIM_ROLE, &json!({"role": "architect"}), 1, &state);
        let parsed: Value = serde_json::from_str(&result.content[0].text).unwrap();
        assert_eq!(parsed["claimed"], false);
    }

    #[test]
    fn claim_role_idempotent_same_pane() {
        let state = make_state();
        handle_tool_call(TOOL_CLAIM_ROLE, &json!({"role": "architect"}), 0, &state);
        let result = handle_tool_call(TOOL_CLAIM_ROLE, &json!({"role": "architect", "description": "updated"}), 0, &state);
        let parsed: Value = serde_json::from_str(&result.content[0].text).unwrap();
        assert_eq!(parsed["claimed"], true);
    }

    #[test]
    fn claim_role_missing_name() {
        let state = make_state();
        let result = handle_tool_call(TOOL_CLAIM_ROLE, &json!({}), 0, &state);
        assert!(result.is_error);
    }

    #[test]
    fn claim_different_roles_succeeds() {
        let state = make_state();
        handle_tool_call(TOOL_CLAIM_ROLE, &json!({"role": "architect"}), 0, &state);
        let result = handle_tool_call(TOOL_CLAIM_ROLE, &json!({"role": "implementer"}), 1, &state);
        let parsed: Value = serde_json::from_str(&result.content[0].text).unwrap();
        assert_eq!(parsed["claimed"], true);
        // Both roles should appear in all_roles.
        let all = parsed["all_roles"].as_array().unwrap();
        assert_eq!(all.len(), 2);
    }

    // ── potato_get_role ───────────────────────────────────────────────────────

    #[test]
    fn get_role_with_assigned_role() {
        let state = make_state_with_roles();
        let result = handle_tool_call(TOOL_GET_ROLE, &json!({}), 0, &state);
        assert!(!result.is_error);
        assert!(result.content[0].text.contains("architect"));
    }

    #[test]
    fn get_role_unassigned() {
        let state = make_state();
        let result = handle_tool_call(TOOL_GET_ROLE, &json!({}), 99, &state);
        assert!(!result.is_error);
        assert!(result.content[0].text.contains("unassigned"));
    }

    #[test]
    fn get_role_includes_partner_roles() {
        let state = make_state_with_roles();
        let result = handle_tool_call(TOOL_GET_ROLE, &json!({}), 0, &state);
        assert!(!result.is_error);
        assert!(result.content[0].text.contains("implementer"));
    }
}
