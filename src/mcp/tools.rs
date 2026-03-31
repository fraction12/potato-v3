//! MCP tool definitions and dispatch for the Potato inter-session server.
//!
//! `TOOL_DEFINITIONS` enumerates all 8 tools (as a `Vec` built once at call time).
//! `handle_tool_call` dispatches a `tools/call` request to the correct state method.

use std::sync::{Arc, Mutex};

use serde_json::{Value, json};

use crate::mcp::protocol::{CallToolResult, ToolInfo};
use crate::mcp::state::{
    ClaimResult, InterSessionState, MessagePriority, PaneRole, RoleClaimResult,
};

// ── Tool names ────────────────────────────────────────────────────────────────

pub const TOOL_SEND_MESSAGE: &str = "potato_send_message";
pub const TOOL_GET_MESSAGES: &str = "potato_get_messages";
pub const TOOL_GET_PARTNER_STATUS: &str = "potato_get_partner_status";
pub const TOOL_SHARED_CONTEXT: &str = "potato_shared_context";
pub const TOOL_CLAIM_TASK: &str = "potato_claim_task";
pub const TOOL_RELEASE_TASK: &str = "potato_release_task";
pub const TOOL_CLAIM_ROLE: &str = "potato_claim_role";
pub const TOOL_GET_ROLE: &str = "potato_get_role";
pub const TOOL_LIST_TASKS: &str = "potato_list_tasks";

// ── Tool definitions ──────────────────────────────────────────────────────────

/// Return all 8 Potato MCP tool definitions with full JSON schemas.
pub fn tool_definitions() -> Vec<ToolInfo> {
    vec![
        ToolInfo {
            name: TOOL_SEND_MESSAGE.into(),
            description: "Send a structured message to another agent session running in Potato. \
                Messages must use the structured format — no markdown allowed anywhere. \
                The message will be delivered to the target pane's agent.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "to": {
                        "type": "string",
                        "description": "Target pane identifier. Use 'partner' for the other pane, or a specific pane ID as a number string."
                    },
                    "type": {
                        "type": "string",
                        "enum": ["task", "status", "question", "result"],
                        "description": "Message type: 'task' for work assignments, 'status' for progress updates, 'question' for queries, 'result' for deliverables."
                    },
                    "subject": {
                        "type": "string",
                        "description": "Short plain-text subject line (max 200 chars). No markdown."
                    },
                    "body": {
                        "type": "object",
                        "description": "Structured message body. No markdown allowed in any field.",
                        "properties": {
                            "summary": {
                                "type": "string",
                                "description": "Plain-text summary of the message (max 500 chars). Required."
                            },
                            "files": {
                                "type": "array",
                                "items": { "type": "string" },
                                "description": "Optional list of relevant file paths."
                            },
                            "steps": {
                                "type": "array",
                                "items": { "type": "string" },
                                "description": "Optional list of action items or steps (each max 200 chars)."
                            },
                            "context": {
                                "type": "string",
                                "description": "Optional additional context (max 1000 chars)."
                            }
                        },
                        "required": ["summary"]
                    },
                    "priority": {
                        "type": "string",
                        "enum": ["normal", "urgent"],
                        "default": "normal",
                        "description": "Urgent messages trigger immediate PTY injection."
                    }
                },
                "required": ["type", "subject", "body"]
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
                        "description": "The role name to claim. Use the role assigned to you by your bootstrap prompt, or choose one that reflects what you are actually doing."
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
        ToolInfo {
            name: TOOL_LIST_TASKS.into(),
            description: "List all open/actionable tasks from the project's OpenSpec changes (openspec/changes/*/tasks.md). \
                Use this to see what tickets are available to work on. Claim a task by ID with potato_claim_task.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "status": {
                        "type": "string",
                        "description": "Optional filter: 'open', 'claimed', 'in-progress', 'blocked'. Default: all non-done."
                    }
                }
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
        TOOL_LIST_TASKS => handle_list_tasks(args, state),
        unknown => CallToolResult::failure(format!("Unknown tool: {unknown}")),
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Lock the shared state, returning a `CallToolResult::failure` on poison.
macro_rules! lock_state {
    ($state:expr) => {
        match $state.lock() {
            Ok(g) => g,
            Err(e) => {
                tracing::error!("InterSessionState mutex poisoned: {e}");
                return CallToolResult::failure("State lock poisoned");
            }
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

/// Markdown markers that are rejected in structured message fields.
const MARKDOWN_MARKERS: &[&str] = &["**", "###", "```"];

/// Check if a string contains markdown markers.
fn contains_markdown(s: &str) -> bool {
    MARKDOWN_MARKERS.iter().any(|m| s.contains(m))
}

/// Validate all fields of a structured message, collecting ALL errors.
fn validate_structured_message(args: &Value) -> Result<(String, String, Value), Vec<String>> {
    let mut errors = Vec::new();

    // type — required, must be one of the allowed values.
    let msg_type = match args.get("type").and_then(Value::as_str) {
        Some(t) if ["task", "status", "question", "result"].contains(&t) => Some(t.to_string()),
        Some(t) => {
            errors.push(format!(
                "Invalid type: '{t}'. Must be one of: task, status, question, result"
            ));
            None
        }
        None => {
            errors.push("Missing required field: type".to_string());
            None
        }
    };

    // subject — required, max 200 chars, plain text only.
    let subject = match args.get("subject").and_then(Value::as_str) {
        Some(s) => {
            if s.len() > 200 {
                errors.push(format!(
                    "subject exceeds 200 chars (got {})",
                    s.len()
                ));
            }
            if contains_markdown(s) {
                errors.push("subject contains markdown (**, ###, or ```). Plain text only.".to_string());
            }
            Some(s.to_string())
        }
        None => {
            errors.push("Missing required field: subject".to_string());
            None
        }
    };

    // body — required object with summary (required) + optional fields.
    let body = match args.get("body") {
        Some(b) if b.is_object() => {
            // body.summary — required, max 500 chars, plain text.
            match b.get("summary").and_then(Value::as_str) {
                Some(s) => {
                    if s.len() > 500 {
                        errors.push(format!(
                            "body.summary exceeds 500 chars (got {})",
                            s.len()
                        ));
                    }
                    if contains_markdown(s) {
                        errors.push("body.summary contains markdown. Plain text only.".to_string());
                    }
                }
                None => {
                    errors.push("Missing required field: body.summary".to_string());
                }
            }

            // body.files — optional array of strings.
            if let Some(files) = b.get("files") {
                if let Some(arr) = files.as_array() {
                    for (i, item) in arr.iter().enumerate() {
                        if !item.is_string() {
                            errors.push(format!("body.files[{i}] must be a string"));
                        }
                    }
                } else {
                    errors.push("body.files must be an array of strings".to_string());
                }
            }

            // body.steps — optional array of strings, each max 200 chars.
            if let Some(steps) = b.get("steps") {
                if let Some(arr) = steps.as_array() {
                    for (i, item) in arr.iter().enumerate() {
                        match item.as_str() {
                            Some(s) => {
                                if s.len() > 200 {
                                    errors.push(format!(
                                        "body.steps[{i}] exceeds 200 chars (got {})",
                                        s.len()
                                    ));
                                }
                                if contains_markdown(s) {
                                    errors.push(format!(
                                        "body.steps[{i}] contains markdown. Plain text only."
                                    ));
                                }
                            }
                            None => {
                                errors.push(format!("body.steps[{i}] must be a string"));
                            }
                        }
                    }
                } else {
                    errors.push("body.steps must be an array of strings".to_string());
                }
            }

            // body.context — optional, max 1000 chars.
            if let Some(ctx) = b.get("context").and_then(Value::as_str) {
                if ctx.len() > 1000 {
                    errors.push(format!(
                        "body.context exceeds 1000 chars (got {})",
                        ctx.len()
                    ));
                }
                if contains_markdown(ctx) {
                    errors.push("body.context contains markdown. Plain text only.".to_string());
                }
            }

            Some(b.clone())
        }
        Some(_) => {
            errors.push("body must be a JSON object".to_string());
            None
        }
        None => {
            errors.push("Missing required field: body".to_string());
            None
        }
    };

    if !errors.is_empty() {
        return Err(errors);
    }

    // All validated — unwrap is safe because errors would have been pushed above.
    Ok((msg_type.unwrap(), subject.unwrap(), body.unwrap()))
}

/// Build the schema hint included in validation error responses.
fn expected_schema_hint() -> &'static str {
    r#"Expected format: { "to": "partner", "type": "task|status|question|result", "subject": "plain text (max 200)", "body": { "summary": "plain text (max 500)", "files": ["path", ...], "steps": ["step (max 200)", ...], "context": "plain text (max 1000)" }, "priority": "normal|urgent" }. No markdown (**, ###, ```) anywhere."#
}

fn handle_send_message(
    args: &Value,
    pane_id: u64,
    state: &Arc<Mutex<InterSessionState>>,
) -> CallToolResult {
    // Validate structured message fields, collecting all errors.
    let (msg_type, subject, body) = match validate_structured_message(args) {
        Ok(validated) => validated,
        Err(errors) => {
            let error_list = errors
                .iter()
                .enumerate()
                .map(|(i, e)| format!("  {}. {e}", i + 1))
                .collect::<Vec<_>>()
                .join("\n");
            return CallToolResult::failure(format!(
                "Validation failed ({} error{}):\n{error_list}\n\n{}\n",
                errors.len(),
                if errors.len() == 1 { "" } else { "s" },
                expected_schema_hint(),
            ));
        }
    };

    let priority = match args.get("priority").and_then(Value::as_str) {
        Some("urgent") => MessagePriority::Urgent,
        Some("normal") | None => MessagePriority::Normal,
        Some(other) => {
            return CallToolResult::failure(format!(
                "Invalid priority: {other}. Must be 'normal' or 'urgent'"
            ));
        }
    };

    // Resolve target and send in a single lock acquisition to avoid TOCTOU.
    let to_explicit: Option<u64> = match args.get("to").and_then(Value::as_str) {
        Some("partner") | None => None,
        Some(id_str) => match id_str.parse::<u64>() {
            Ok(id) => Some(id),
            Err(_) => return CallToolResult::failure(format!("Invalid target pane id: {id_str}")),
        },
    };

    // Serialize validated message as JSON for storage in InterMessage.content.
    let structured_content = json!({
        "type": msg_type,
        "subject": subject,
        "body": body,
    });
    let content_json = serde_json::to_string(&structured_content).unwrap_or_default();

    let mut st = lock_state!(state);
    let to_pane = match to_explicit {
        Some(id) => id,
        None => match st.resolve_partner(pane_id) {
            Some(partner) => partner,
            None => return CallToolResult::failure("No partner pane found."),
        },
    };

    if !st.send_message(pane_id, to_pane, content_json, priority) {
        return CallToolResult::failure(format!("Target pane {to_pane} is not registered."));
    }

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

fn handle_shared_context(args: &Value, state: &Arc<Mutex<InterSessionState>>) -> CallToolResult {
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
                None => {
                    return CallToolResult::failure("Missing required field: value (for op=set)");
                }
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
                None => {
                    return CallToolResult::failure("Missing required field: key (for op=delete)");
                }
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

    let role = PaneRole {
        name: role_name.clone(),
        description,
    };
    match st.claim_role(pane_id, role) {
        RoleClaimResult::Claimed => {
            let all_roles = collect_all_roles(&st, None);
            CallToolResult::success(
                serde_json::to_string_pretty(&json!({
                    "claimed": true,
                    "role": role_name,
                    "all_roles": all_roles
                }))
                .unwrap_or_default(),
            )
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

fn handle_get_role(pane_id: u64, state: &Arc<Mutex<InterSessionState>>) -> CallToolResult {
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

fn handle_list_tasks(args: &Value, state: &Arc<Mutex<InterSessionState>>) -> CallToolResult {
    let st = lock_state!(state);
    let status_filter = args.get("status").and_then(|v| v.as_str());

    let tasks: Vec<&crate::mcp::state::OpenSpecTaskSnapshot> = st
        .openspec_tasks
        .iter()
        .filter(|t| match status_filter {
            Some(filter) => t.status == filter,
            None => true,
        })
        .collect();

    // Annotate with claim info from the task board.
    let annotated: Vec<Value> = tasks
        .iter()
        .map(|t| {
            let claimed_by = st.task_board.get(&t.id).map(|c| c.claimed_by);
            json!({
                "id": t.id,
                "title": t.title,
                "status": t.status,
                "phase": t.phase,
                "severity": t.severity,
                "claimed_by_pane": claimed_by,
            })
        })
        .collect();

    let result = json!({
        "total": annotated.len(),
        "tasks": annotated,
    });

    CallToolResult::success(serde_json::to_string_pretty(&result).unwrap_or_default())
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn make_state() -> Arc<Mutex<InterSessionState>> {
        let state = Arc::new(Mutex::new(InterSessionState::new()));
        {
            let mut st = state.lock().unwrap();
            st.register_pane(0);
            st.register_pane(1);
        }
        state
    }

    fn make_state_with_roles() -> Arc<Mutex<InterSessionState>> {
        let state = make_state();
        {
            let mut st = state.lock().unwrap();
            st.set_role(
                0,
                PaneRole {
                    name: "architect".into(),
                    description: "Designs systems".into(),
                },
            );
            st.set_role(
                1,
                PaneRole {
                    name: "implementer".into(),
                    description: "Builds things".into(),
                },
            );
        }
        state
    }

    // ── tool_definitions ─────────────────────────────────────────────────────

    #[test]
    fn tool_definitions_returns_all_tools() {
        let tools = tool_definitions();
        assert_eq!(tools.len(), 9); // 6 spec tools + claim_role + get_role + list_tasks
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
            assert!(
                !tool.description.is_empty(),
                "Tool {} has empty description",
                tool.name
            );
        }
    }

    #[test]
    fn all_tools_have_object_input_schema() {
        for tool in tool_definitions() {
            assert_eq!(
                tool.input_schema["type"], "object",
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

    /// Helper to build a valid structured message JSON.
    fn structured_msg(subject: &str, summary: &str) -> Value {
        json!({
            "type": "status",
            "subject": subject,
            "body": { "summary": summary }
        })
    }

    /// Helper to build a structured message with all optional fields.
    fn full_structured_msg() -> Value {
        json!({
            "type": "task",
            "subject": "T-100: Wire up profiles",
            "body": {
                "summary": "ProfileLoader exists but is never called.",
                "files": ["src/config/profiles.rs", "src/app/state.rs"],
                "steps": ["Rename profiles.toml to agents.toml", "Feed into AppState"],
                "context": "This is background context."
            }
        })
    }

    #[test]
    fn send_message_basic() {
        let state = make_state();
        let args = structured_msg("hello partner", "greeting summary");
        let result = handle_tool_call(TOOL_SEND_MESSAGE, &args, 0, &state);
        assert!(!result.is_error);
        // Verify message arrived in inbox of pane 1 as JSON.
        let msgs = state.lock().unwrap().get_messages(1, false);
        assert_eq!(msgs.len(), 1);
        let parsed: Value = serde_json::from_str(&msgs[0].content).unwrap();
        assert_eq!(parsed["type"], "status");
        assert_eq!(parsed["subject"], "hello partner");
        assert_eq!(parsed["body"]["summary"], "greeting summary");
    }

    #[test]
    fn send_message_to_specific_pane() {
        let state = make_state();
        state.lock().unwrap().register_pane(2);
        let mut args = structured_msg("direct", "direct summary");
        args["to"] = json!("2");
        let result = handle_tool_call(TOOL_SEND_MESSAGE, &args, 0, &state);
        assert!(!result.is_error);
        let msgs = state.lock().unwrap().get_messages(2, false);
        assert_eq!(msgs.len(), 1);
    }

    #[test]
    fn send_message_urgent_priority() {
        let state = make_state();
        let mut args = structured_msg("urgent subject", "urgent summary");
        args["priority"] = json!("urgent");
        let result = handle_tool_call(TOOL_SEND_MESSAGE, &args, 0, &state);
        assert!(!result.is_error);
        let msgs = state.lock().unwrap().get_messages(1, false);
        assert_eq!(msgs[0].priority, MessagePriority::Urgent);
    }

    #[test]
    fn send_message_missing_required_fields() {
        let state = make_state();
        let result = handle_tool_call(TOOL_SEND_MESSAGE, &json!({"to": "1"}), 0, &state);
        assert!(result.is_error);
        let text = &result.content[0].text;
        assert!(text.contains("Missing required field: type"));
        assert!(text.contains("Missing required field: subject"));
        assert!(text.contains("Missing required field: body"));
    }

    #[test]
    fn send_message_invalid_priority() {
        let state = make_state();
        let mut args = structured_msg("test", "test summary");
        args["priority"] = json!("super_urgent");
        let result = handle_tool_call(TOOL_SEND_MESSAGE, &args, 0, &state);
        assert!(result.is_error);
        assert!(result.content[0].text.contains("Invalid priority"));
    }

    #[test]
    fn send_message_invalid_pane_id() {
        let state = make_state();
        let mut args = structured_msg("test", "test summary");
        args["to"] = json!("not_a_number");
        let result = handle_tool_call(TOOL_SEND_MESSAGE, &args, 0, &state);
        assert!(result.is_error);
    }

    #[test]
    fn send_message_invalid_type() {
        let state = make_state();
        let args = json!({
            "type": "announcement",
            "subject": "test",
            "body": { "summary": "test" }
        });
        let result = handle_tool_call(TOOL_SEND_MESSAGE, &args, 0, &state);
        assert!(result.is_error);
        assert!(result.content[0].text.contains("Invalid type: 'announcement'"));
    }

    #[test]
    fn send_message_rejects_markdown_in_subject() {
        let state = make_state();
        let args = json!({
            "type": "task",
            "subject": "**bold subject**",
            "body": { "summary": "clean summary" }
        });
        let result = handle_tool_call(TOOL_SEND_MESSAGE, &args, 0, &state);
        assert!(result.is_error);
        assert!(result.content[0].text.contains("subject contains markdown"));
    }

    #[test]
    fn send_message_rejects_markdown_in_summary() {
        let state = make_state();
        let args = json!({
            "type": "task",
            "subject": "clean subject",
            "body": { "summary": "### heading in summary" }
        });
        let result = handle_tool_call(TOOL_SEND_MESSAGE, &args, 0, &state);
        assert!(result.is_error);
        assert!(result.content[0].text.contains("body.summary contains markdown"));
    }

    #[test]
    fn send_message_rejects_markdown_in_steps() {
        let state = make_state();
        let args = json!({
            "type": "task",
            "subject": "clean",
            "body": { "summary": "clean", "steps": ["step 1", "```code block```"] }
        });
        let result = handle_tool_call(TOOL_SEND_MESSAGE, &args, 0, &state);
        assert!(result.is_error);
        assert!(result.content[0].text.contains("body.steps[1] contains markdown"));
    }

    #[test]
    fn send_message_subject_length_limit() {
        let state = make_state();
        let long_subject = "x".repeat(201);
        let args = json!({
            "type": "status",
            "subject": long_subject,
            "body": { "summary": "ok" }
        });
        let result = handle_tool_call(TOOL_SEND_MESSAGE, &args, 0, &state);
        assert!(result.is_error);
        assert!(result.content[0].text.contains("subject exceeds 200 chars"));
    }

    #[test]
    fn send_message_summary_length_limit() {
        let state = make_state();
        let long_summary = "x".repeat(501);
        let args = json!({
            "type": "status",
            "subject": "ok",
            "body": { "summary": long_summary }
        });
        let result = handle_tool_call(TOOL_SEND_MESSAGE, &args, 0, &state);
        assert!(result.is_error);
        assert!(result.content[0].text.contains("body.summary exceeds 500 chars"));
    }

    #[test]
    fn send_message_context_length_limit() {
        let state = make_state();
        let long_ctx = "x".repeat(1001);
        let args = json!({
            "type": "status",
            "subject": "ok",
            "body": { "summary": "ok", "context": long_ctx }
        });
        let result = handle_tool_call(TOOL_SEND_MESSAGE, &args, 0, &state);
        assert!(result.is_error);
        assert!(result.content[0].text.contains("body.context exceeds 1000 chars"));
    }

    #[test]
    fn send_message_step_length_limit() {
        let state = make_state();
        let long_step = "x".repeat(201);
        let args = json!({
            "type": "task",
            "subject": "ok",
            "body": { "summary": "ok", "steps": [long_step] }
        });
        let result = handle_tool_call(TOOL_SEND_MESSAGE, &args, 0, &state);
        assert!(result.is_error);
        assert!(result.content[0].text.contains("body.steps[0] exceeds 200 chars"));
    }

    #[test]
    fn send_message_reports_all_errors() {
        let state = make_state();
        let args = json!({
            "type": "invalid",
            "subject": "**markdown**",
            "body": { "summary": "### also markdown" }
        });
        let result = handle_tool_call(TOOL_SEND_MESSAGE, &args, 0, &state);
        assert!(result.is_error);
        let text = &result.content[0].text;
        // Should report all three errors, not just the first.
        assert!(text.contains("Invalid type"));
        assert!(text.contains("subject contains markdown"));
        assert!(text.contains("body.summary contains markdown"));
        assert!(text.contains("3 errors"));
    }

    #[test]
    fn send_message_full_structured() {
        let state = make_state();
        let args = full_structured_msg();
        let result = handle_tool_call(TOOL_SEND_MESSAGE, &args, 0, &state);
        assert!(!result.is_error);
        let msgs = state.lock().unwrap().get_messages(1, false);
        let parsed: Value = serde_json::from_str(&msgs[0].content).unwrap();
        assert_eq!(parsed["type"], "task");
        assert_eq!(parsed["body"]["files"].as_array().unwrap().len(), 2);
        assert_eq!(parsed["body"]["steps"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn send_message_body_not_object() {
        let state = make_state();
        let args = json!({
            "type": "status",
            "subject": "test",
            "body": "not an object"
        });
        let result = handle_tool_call(TOOL_SEND_MESSAGE, &args, 0, &state);
        assert!(result.is_error);
        assert!(result.content[0].text.contains("body must be a JSON object"));
    }

    #[test]
    fn send_message_missing_body_summary() {
        let state = make_state();
        let args = json!({
            "type": "status",
            "subject": "test",
            "body": { "files": ["a.rs"] }
        });
        let result = handle_tool_call(TOOL_SEND_MESSAGE, &args, 0, &state);
        assert!(result.is_error);
        assert!(result.content[0].text.contains("Missing required field: body.summary"));
    }

    #[test]
    fn send_message_error_includes_schema_hint() {
        let state = make_state();
        let result = handle_tool_call(TOOL_SEND_MESSAGE, &json!({}), 0, &state);
        assert!(result.is_error);
        assert!(result.content[0].text.contains("Expected format:"));
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
        state
            .lock()
            .unwrap()
            .send_message(1, 0, "from pane 1", MessagePriority::Normal);
        let result = handle_tool_call(TOOL_GET_MESSAGES, &json!({}), 0, &state);
        assert!(!result.is_error);
        assert!(result.content[0].text.contains("from pane 1"));
    }

    #[test]
    fn get_messages_marks_read_by_default() {
        let state = make_state();
        state
            .lock()
            .unwrap()
            .send_message(1, 0, "msg", MessagePriority::Normal);
        handle_tool_call(TOOL_GET_MESSAGES, &json!({}), 0, &state);
        // Second call should return empty.
        let result2 = handle_tool_call(TOOL_GET_MESSAGES, &json!({}), 0, &state);
        assert!(result2.content[0].text.contains("No unread messages"));
    }

    #[test]
    fn get_messages_does_not_mark_read_when_false() {
        let state = make_state();
        state
            .lock()
            .unwrap()
            .send_message(1, 0, "msg", MessagePriority::Normal);
        handle_tool_call(TOOL_GET_MESSAGES, &json!({"mark_read": false}), 0, &state);
        // Should still be unread.
        let result2 = handle_tool_call(TOOL_GET_MESSAGES, &json!({"mark_read": false}), 0, &state);
        assert!(result2.content[0].text.contains("msg"));
    }

    // ── potato_get_partner_status ─────────────────────────────────────────────

    #[test]
    fn get_partner_status_no_partners() {
        // Only register one pane so there are genuinely no partners.
        let state = Arc::new(Mutex::new(InterSessionState::new()));
        state.lock().unwrap().register_pane(0);
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
        handle_tool_call(
            TOOL_SHARED_CONTEXT,
            &json!({"op": "set", "key": "k", "value": "v"}),
            0,
            &state,
        );
        let result = handle_tool_call(
            TOOL_SHARED_CONTEXT,
            &json!({"op": "get", "key": "k"}),
            0,
            &state,
        );
        assert!(!result.is_error);
        assert!(result.content[0].text.contains("v"));
    }

    #[test]
    fn shared_context_get_missing_key() {
        let state = make_state();
        let result = handle_tool_call(
            TOOL_SHARED_CONTEXT,
            &json!({"op": "get", "key": "nope"}),
            0,
            &state,
        );
        assert!(!result.is_error);
        assert!(result.content[0].text.contains("not found"));
    }

    #[test]
    fn shared_context_delete_existing() {
        let state = make_state();
        handle_tool_call(
            TOOL_SHARED_CONTEXT,
            &json!({"op": "set", "key": "k", "value": 1}),
            0,
            &state,
        );
        let result = handle_tool_call(
            TOOL_SHARED_CONTEXT,
            &json!({"op": "delete", "key": "k"}),
            0,
            &state,
        );
        assert!(!result.is_error);
        assert!(result.content[0].text.contains("Deleted"));
    }

    #[test]
    fn shared_context_delete_missing() {
        let state = make_state();
        let result = handle_tool_call(
            TOOL_SHARED_CONTEXT,
            &json!({"op": "delete", "key": "ghost"}),
            0,
            &state,
        );
        assert!(!result.is_error);
        assert!(result.content[0].text.contains("not found"));
    }

    #[test]
    fn shared_context_list() {
        let state = make_state();
        handle_tool_call(
            TOOL_SHARED_CONTEXT,
            &json!({"op": "set", "key": "b", "value": 1}),
            0,
            &state,
        );
        handle_tool_call(
            TOOL_SHARED_CONTEXT,
            &json!({"op": "set", "key": "a", "value": 2}),
            0,
            &state,
        );
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
        assert!(
            result.content[0]
                .text
                .contains("Missing required field: op")
        );
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
        let result = handle_tool_call(
            TOOL_SHARED_CONTEXT,
            &json!({"op": "set", "value": 1}),
            0,
            &state,
        );
        assert!(result.is_error);
        assert!(
            result.content[0]
                .text
                .contains("Missing required field: key")
        );
    }

    #[test]
    fn shared_context_set_missing_value() {
        let state = make_state();
        let result = handle_tool_call(
            TOOL_SHARED_CONTEXT,
            &json!({"op": "set", "key": "k"}),
            0,
            &state,
        );
        assert!(result.is_error);
        assert!(
            result.content[0]
                .text
                .contains("Missing required field: value")
        );
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
        assert!(
            result.content[0]
                .text
                .contains("Missing required field: task_id")
        );
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
        assert!(
            result.content[0]
                .text
                .contains("Missing required field: task_id")
        );
    }

    // ── potato_claim_role ──────────────────────────────────────────────────────

    #[test]
    fn claim_role_success() {
        let state = make_state();
        let result = handle_tool_call(
            TOOL_CLAIM_ROLE,
            &json!({"role": "architect", "description": "Designs"}),
            0,
            &state,
        );
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
        let result = handle_tool_call(
            TOOL_CLAIM_ROLE,
            &json!({"role": "architect", "description": "updated"}),
            0,
            &state,
        );
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
