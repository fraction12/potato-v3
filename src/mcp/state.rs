//! Inter-session shared state for the Potato MCP server.
//!
//! `InterSessionState` is the in-memory shared state that all MCP server
//! instances (one per pane) read and write through.

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::project_store::ProjectStore;

// ── Domain types ──────────────────────────────────────────────────────────────

/// Priority level for inter-session messages.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MessagePriority {
    Normal,
    Urgent,
}

impl Default for MessagePriority {
    fn default() -> Self {
        Self::Normal
    }
}

/// A message in a pane's inbox.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InterMessage {
    pub from_pane: u64,
    pub content: String,
    pub priority: MessagePriority,
    pub timestamp: DateTime<Utc>,
    pub read: bool,
}

/// Records that a pane has claimed a task.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskClaim {
    pub task_id: String,
    pub description: String,
    pub claimed_by: u64,
    pub claimed_at: DateTime<Utc>,
}

/// Role assignment for a pane.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PaneRole {
    pub name: String,
    pub description: String,
}

/// Result of attempting to claim a role.
#[derive(Debug, Clone, PartialEq)]
pub enum RoleClaimResult {
    /// Role was claimed successfully.
    Claimed,
    /// Role name is already held by another pane.
    AlreadyClaimed { held_by: u64 },
}

// ── Claim result ──────────────────────────────────────────────────────────────

/// Result of attempting to claim a task.
#[derive(Debug, Clone, PartialEq)]
pub enum ClaimResult {
    /// Task was claimed successfully.
    Claimed,
    /// Task is already held by another pane.
    AlreadyClaimed { held_by: u64, since: DateTime<Utc> },
}

// ── InterSessionState ─────────────────────────────────────────────────────────

/// All shared state for the inter-session MCP layer.
///
/// Lives in Potato's main process; accessed by MCP server instances via
/// `Arc<Mutex<InterSessionState>>`.
#[derive(Debug, Default)]
pub struct InterSessionState {
    /// Per-pane message inboxes.
    pub inboxes: HashMap<u64, VecDeque<InterMessage>>,

    /// Shared key-value working memory visible to all panes.
    pub shared_context: HashMap<String, Value>,

    /// Mutex-style task coordination board.
    pub task_board: HashMap<String, TaskClaim>,

    /// Role assignments per pane.
    pub roles: HashMap<u64, PaneRole>,

    /// Currently known live pane IDs.
    ///
    /// Maintained by the main loop so partner resolution doesn't depend
    /// on fragile assumptions like `id ^ 1`.
    pub known_panes: Vec<u64>,

    /// Optional project-scoped persistent backing store.
    /// When present, shared_context and task_board mutations write through.
    /// Behind a Mutex because `rusqlite::Connection` is Send but not Sync.
    pub backing_store: Option<Arc<std::sync::Mutex<ProjectStore>>>,

    /// Pending task events for the main loop to sync to OpenSpec.
    /// Drained by the main loop each tick.
    pub pending_task_events: Vec<TaskEvent>,

    /// Snapshot of OpenSpec backlog tasks (refreshed by main loop).
    /// Agents can read this via `potato_list_tasks`.
    pub openspec_tasks: Vec<OpenSpecTaskSnapshot>,
}

/// Task lifecycle events emitted by claim/release for external sync (e.g. OpenSpec).
#[derive(Debug, Clone)]
pub enum TaskEvent {
    Claimed { task_id: String, pane_id: u64 },
    Released { task_id: String },
}

/// Lightweight snapshot of an OpenSpec task for MCP tool access.
#[derive(Debug, Clone, Serialize)]
pub struct OpenSpecTaskSnapshot {
    pub id: String,
    pub title: String,
    pub status: String,
    pub phase: Option<String>,
    pub severity: Option<String>,
}

impl InterSessionState {
    /// Create an empty inter-session state with no backing store.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create with a backing store and hydrate from it.
    pub fn with_store(store: Arc<std::sync::Mutex<ProjectStore>>) -> Self {
        let mut state = Self {
            backing_store: Some(Arc::clone(&store)),
            ..Self::default()
        };

        // Hydrate from backing store.
        if let Ok(s) = store.lock() {
            if let Ok(entries) = s.load_context() {
                for (key, value) in entries {
                    state.shared_context.insert(key, value);
                }
            }
            if let Ok(tasks) = s.load_tasks() {
                for task in tasks {
                    state.task_board.insert(task.task_id.clone(), task);
                }
            }
        }

        state
    }

    /// Drain pending task events (called by the main loop each tick).
    pub fn drain_task_events(&mut self) -> Vec<TaskEvent> {
        std::mem::take(&mut self.pending_task_events)
    }

    /// Register a pane as live.
    pub fn register_pane(&mut self, pane_id: u64) {
        if !self.known_panes.contains(&pane_id) {
            self.known_panes.push(pane_id);
        }
    }

    /// Remove a pane (on close/death).
    ///
    /// Cleans up all associated state: role, inbox, and task claims.
    pub fn unregister_pane(&mut self, pane_id: u64) {
        self.known_panes.retain(|&id| id != pane_id);
        self.roles.remove(&pane_id);
        self.inboxes.remove(&pane_id);
        self.task_board
            .retain(|_, claim| claim.claimed_by != pane_id);
    }

    /// Find the partner pane ID for `pane_id`.
    ///
    /// Returns the first known pane that isn't `pane_id`, or `None`.
    pub fn resolve_partner(&self, pane_id: u64) -> Option<u64> {
        self.known_panes.iter().find(|&&id| id != pane_id).copied()
    }

    // ── Messaging ─────────────────────────────────────────────────────────────

    /// Enqueue a message into `to_pane`'s inbox.
    pub fn send_message(
        &mut self,
        from_pane: u64,
        to_pane: u64,
        content: impl Into<String>,
        priority: MessagePriority,
    ) {
        let msg = InterMessage {
            from_pane,
            content: content.into(),
            priority,
            timestamp: Utc::now(),
            read: false,
        };
        // Log to persistent store for audit/history.
        if let Some(ref store) = self.backing_store {
            if let Ok(s) = store.lock() {
                if let Err(e) = s.log_message(&msg, to_pane) {
                    tracing::warn!("Failed to log message to project store: {e}");
                }
            }
        }
        self.inboxes.entry(to_pane).or_default().push_back(msg);
    }

    /// Drain unread messages from `pane_id`'s inbox.
    ///
    /// If `mark_read` is true, messages are marked read before returning;
    /// they remain in the queue (so they can still be inspected).
    /// Unread-only messages are returned.
    pub fn get_messages(&mut self, pane_id: u64, mark_read: bool) -> Vec<InterMessage> {
        let queue = self.inboxes.entry(pane_id).or_default();
        let unread: Vec<InterMessage> = queue.iter().filter(|m| !m.read).cloned().collect();

        if mark_read {
            for msg in queue.iter_mut() {
                if !msg.read {
                    msg.read = true;
                }
            }
        }

        unread
    }

    // ── Roles ─────────────────────────────────────────────────────────────────

    /// Attempt to claim a role. If the role name is already held by a
    /// different pane, the claim is rejected. Same pane re-claiming the
    /// same role name is idempotent (updates description).
    pub fn claim_role(&mut self, pane_id: u64, role: PaneRole) -> RoleClaimResult {
        // Check if any other pane already holds this role name.
        for (&existing_pane, existing_role) in &self.roles {
            if existing_pane != pane_id && existing_role.name.eq_ignore_ascii_case(&role.name) {
                return RoleClaimResult::AlreadyClaimed {
                    held_by: existing_pane,
                };
            }
        }
        self.roles.insert(pane_id, role);
        RoleClaimResult::Claimed
    }

    /// Assign a role to a pane unconditionally (for internal/slash-command use).
    pub fn set_role(&mut self, pane_id: u64, role: PaneRole) {
        self.roles.insert(pane_id, role);
    }

    /// Get the role assigned to a pane, if any.
    pub fn get_role(&self, pane_id: u64) -> Option<&PaneRole> {
        self.roles.get(&pane_id)
    }

    /// List all currently claimed roles.
    pub fn list_roles(&self) -> Vec<(u64, &PaneRole)> {
        self.roles.iter().map(|(&id, r)| (id, r)).collect()
    }

    /// Get summary status of all panes except `exclude_pane_id`.
    pub fn get_partner_status(&self, exclude_pane_id: u64) -> Vec<PartnerStatus> {
        self.roles
            .iter()
            .filter(|(id, _)| **id != exclude_pane_id)
            .map(|(id, role)| PartnerStatus {
                pane_id: *id,
                role: role.clone(),
                unread_messages: self
                    .inboxes
                    .get(id)
                    .map(|q| q.iter().filter(|m| !m.read).count())
                    .unwrap_or(0),
            })
            .collect()
    }

    // ── Shared context ────────────────────────────────────────────────────────

    /// Read a value from shared context.
    pub fn shared_context_get(&self, key: &str) -> Option<&Value> {
        self.shared_context.get(key)
    }

    /// Write a value to shared context (write-through to backing store).
    pub fn shared_context_set(&mut self, key: impl Into<String>, value: Value) {
        let key = key.into();
        if let Some(ref store) = self.backing_store {
            if let Ok(s) = store.lock() {
                if let Err(e) = s.set_context(&key, &value) {
                    tracing::warn!("Failed to persist shared context key '{key}': {e}");
                }
            }
        }
        self.shared_context.insert(key, value);
    }

    /// Delete a key from shared context (write-through to backing store).
    pub fn shared_context_delete(&mut self, key: &str) -> bool {
        if let Some(ref store) = self.backing_store {
            if let Ok(s) = store.lock() {
                if let Err(e) = s.delete_context(key) {
                    tracing::warn!("Failed to delete shared context key '{key}': {e}");
                }
            }
        }
        self.shared_context.remove(key).is_some()
    }

    /// List all keys in shared context.
    pub fn shared_context_list(&self) -> Vec<String> {
        let mut keys: Vec<String> = self.shared_context.keys().cloned().collect();
        keys.sort();
        keys
    }

    // ── Task board ────────────────────────────────────────────────────────────

    /// Attempt to claim a task. Returns `Claimed` if successful,
    /// or `AlreadyClaimed` if another pane holds it.
    pub fn claim_task(
        &mut self,
        task_id: impl Into<String>,
        description: impl Into<String>,
        pane_id: u64,
    ) -> ClaimResult {
        let task_id = task_id.into();
        if let Some(existing) = self.task_board.get(&task_id) {
            if existing.claimed_by != pane_id {
                return ClaimResult::AlreadyClaimed {
                    held_by: existing.claimed_by,
                    since: existing.claimed_at,
                };
            }
            // Same pane re-claiming — allow (idempotent).
        }
        let description = description.into();
        if let Some(ref store) = self.backing_store {
            if let Ok(s) = store.lock() {
                if let Err(e) = s.upsert_task(&task_id, &description, pane_id) {
                    tracing::warn!("Failed to persist task '{task_id}': {e}");
                }
            }
        }
        self.pending_task_events.push(TaskEvent::Claimed {
            task_id: task_id.clone(),
            pane_id,
        });
        self.task_board.insert(
            task_id.clone(),
            TaskClaim {
                task_id,
                description,
                claimed_by: pane_id,
                claimed_at: Utc::now(),
            },
        );
        ClaimResult::Claimed
    }

    /// Release a task claimed by `pane_id`. Returns true if released,
    /// false if task doesn't exist or is held by a different pane.
    pub fn release_task(&mut self, task_id: &str, pane_id: u64) -> bool {
        match self.task_board.get(task_id) {
            Some(claim) if claim.claimed_by == pane_id => {
                if let Some(ref store) = self.backing_store {
                    if let Ok(s) = store.lock() {
                        if let Err(e) = s.release_task(task_id) {
                            tracing::warn!("Failed to persist task release '{task_id}': {e}");
                        }
                    }
                }
                self.pending_task_events.push(TaskEvent::Released {
                    task_id: task_id.to_string(),
                });
                self.task_board.remove(task_id);
                true
            }
            _ => false,
        }
    }
}

/// Status summary for a partner pane.
#[derive(Debug, Clone)]
pub struct PartnerStatus {
    pub pane_id: u64,
    pub role: PaneRole,
    pub unread_messages: usize,
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn make_state() -> InterSessionState {
        InterSessionState::new()
    }

    // ── Messaging ─────────────────────────────────────────────────────────────

    #[test]
    fn send_and_receive_message() {
        let mut state = make_state();
        state.send_message(0, 1, "hello from pane 0", MessagePriority::Normal);
        let msgs = state.get_messages(1, false);
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].content, "hello from pane 0");
        assert_eq!(msgs[0].from_pane, 0);
        assert!(!msgs[0].read);
    }

    #[test]
    fn messages_are_not_marked_read_when_flag_false() {
        let mut state = make_state();
        state.send_message(0, 1, "msg", MessagePriority::Normal);
        let _ = state.get_messages(1, false);
        // Second call should still return the message as unread.
        let msgs = state.get_messages(1, false);
        assert_eq!(msgs.len(), 1);
    }

    #[test]
    fn messages_are_marked_read_when_flag_true() {
        let mut state = make_state();
        state.send_message(0, 1, "msg", MessagePriority::Normal);
        let _ = state.get_messages(1, true);
        // After marking read, no unread messages.
        let msgs = state.get_messages(1, false);
        assert_eq!(msgs.len(), 0);
    }

    #[test]
    fn multiple_messages_delivered_in_order() {
        let mut state = make_state();
        state.send_message(0, 1, "first", MessagePriority::Normal);
        state.send_message(0, 1, "second", MessagePriority::Normal);
        state.send_message(0, 1, "third", MessagePriority::Urgent);
        let msgs = state.get_messages(1, false);
        assert_eq!(msgs.len(), 3);
        assert_eq!(msgs[0].content, "first");
        assert_eq!(msgs[1].content, "second");
        assert_eq!(msgs[2].content, "third");
        assert_eq!(msgs[2].priority, MessagePriority::Urgent);
    }

    #[test]
    fn messages_dont_cross_pane_inboxes() {
        let mut state = make_state();
        state.send_message(0, 1, "for pane 1", MessagePriority::Normal);
        state.send_message(1, 0, "for pane 0", MessagePriority::Normal);
        let msgs_for_0 = state.get_messages(0, false);
        let msgs_for_1 = state.get_messages(1, false);
        assert_eq!(msgs_for_0.len(), 1);
        assert_eq!(msgs_for_0[0].content, "for pane 0");
        assert_eq!(msgs_for_1.len(), 1);
        assert_eq!(msgs_for_1[0].content, "for pane 1");
    }

    #[test]
    fn empty_inbox_returns_empty_vec() {
        let mut state = make_state();
        let msgs = state.get_messages(99, false);
        assert!(msgs.is_empty());
    }

    // ── Roles ─────────────────────────────────────────────────────────────────

    #[test]
    fn set_and_get_role() {
        let mut state = make_state();
        let role = PaneRole {
            name: "architect".into(),
            description: "Designs the system".into(),
        };
        state.set_role(0, role.clone());
        assert_eq!(state.get_role(0), Some(&role));
    }

    #[test]
    fn get_role_returns_none_if_unset() {
        let state = make_state();
        assert!(state.get_role(42).is_none());
    }

    #[test]
    fn claim_role_succeeds_when_unclaimed() {
        let mut state = make_state();
        let role = PaneRole {
            name: "architect".into(),
            description: "Designs".into(),
        };
        let result = state.claim_role(0, role);
        assert_eq!(result, RoleClaimResult::Claimed);
        assert_eq!(state.get_role(0).unwrap().name, "architect");
    }

    #[test]
    fn claim_role_rejected_when_taken_by_other() {
        let mut state = make_state();
        state.claim_role(
            0,
            PaneRole {
                name: "architect".into(),
                description: "".into(),
            },
        );
        let result = state.claim_role(
            1,
            PaneRole {
                name: "architect".into(),
                description: "".into(),
            },
        );
        assert_eq!(result, RoleClaimResult::AlreadyClaimed { held_by: 0 });
        // Pane 1 should not have a role.
        assert!(state.get_role(1).is_none());
    }

    #[test]
    fn claim_role_case_insensitive_rejection() {
        let mut state = make_state();
        state.claim_role(
            0,
            PaneRole {
                name: "Architect".into(),
                description: "".into(),
            },
        );
        let result = state.claim_role(
            1,
            PaneRole {
                name: "architect".into(),
                description: "".into(),
            },
        );
        assert_eq!(result, RoleClaimResult::AlreadyClaimed { held_by: 0 });
    }

    #[test]
    fn claim_role_idempotent_same_pane() {
        let mut state = make_state();
        state.claim_role(
            0,
            PaneRole {
                name: "architect".into(),
                description: "v1".into(),
            },
        );
        let result = state.claim_role(
            0,
            PaneRole {
                name: "architect".into(),
                description: "v2".into(),
            },
        );
        assert_eq!(result, RoleClaimResult::Claimed);
        assert_eq!(state.get_role(0).unwrap().description, "v2");
    }

    #[test]
    fn claim_different_roles_both_succeed() {
        let mut state = make_state();
        assert_eq!(
            state.claim_role(
                0,
                PaneRole {
                    name: "architect".into(),
                    description: "".into()
                }
            ),
            RoleClaimResult::Claimed
        );
        assert_eq!(
            state.claim_role(
                1,
                PaneRole {
                    name: "implementer".into(),
                    description: "".into()
                }
            ),
            RoleClaimResult::Claimed
        );
    }

    #[test]
    fn set_role_overwrites() {
        let mut state = make_state();
        state.set_role(
            0,
            PaneRole {
                name: "a".into(),
                description: "".into(),
            },
        );
        state.set_role(
            0,
            PaneRole {
                name: "b".into(),
                description: "".into(),
            },
        );
        assert_eq!(state.get_role(0).unwrap().name, "b");
    }

    #[test]
    fn get_partner_status_excludes_self() {
        let mut state = make_state();
        state.set_role(
            0,
            PaneRole {
                name: "architect".into(),
                description: "".into(),
            },
        );
        state.set_role(
            1,
            PaneRole {
                name: "implementer".into(),
                description: "".into(),
            },
        );
        let statuses = state.get_partner_status(0);
        assert_eq!(statuses.len(), 1);
        assert_eq!(statuses[0].pane_id, 1);
        assert_eq!(statuses[0].role.name, "implementer");
    }

    #[test]
    fn get_partner_status_includes_unread_count() {
        let mut state = make_state();
        state.set_role(
            1,
            PaneRole {
                name: "implementer".into(),
                description: "".into(),
            },
        );
        state.send_message(0, 1, "a", MessagePriority::Normal);
        state.send_message(0, 1, "b", MessagePriority::Normal);
        let statuses = state.get_partner_status(0);
        // pane 1 has 2 unread messages in ITS inbox
        let pane1 = statuses.iter().find(|s| s.pane_id == 1).unwrap();
        assert_eq!(pane1.unread_messages, 2);
    }

    // ── Shared context ────────────────────────────────────────────────────────

    #[test]
    fn context_set_and_get() {
        let mut state = make_state();
        state.shared_context_set("key1", json!("value1"));
        assert_eq!(state.shared_context_get("key1"), Some(&json!("value1")));
    }

    #[test]
    fn context_get_missing_key() {
        let state = make_state();
        assert!(state.shared_context_get("nope").is_none());
    }

    #[test]
    fn context_delete_existing_key() {
        let mut state = make_state();
        state.shared_context_set("k", json!(1));
        let deleted = state.shared_context_delete("k");
        assert!(deleted);
        assert!(state.shared_context_get("k").is_none());
    }

    #[test]
    fn context_delete_missing_key_returns_false() {
        let mut state = make_state();
        assert!(!state.shared_context_delete("ghost"));
    }

    #[test]
    fn context_list_sorted() {
        let mut state = make_state();
        state.shared_context_set("zebra", json!(1));
        state.shared_context_set("apple", json!(2));
        state.shared_context_set("mango", json!(3));
        let keys = state.shared_context_list();
        assert_eq!(keys, vec!["apple", "mango", "zebra"]);
    }

    #[test]
    fn context_list_empty() {
        let state = make_state();
        assert!(state.shared_context_list().is_empty());
    }

    #[test]
    fn context_set_overwrites() {
        let mut state = make_state();
        state.shared_context_set("k", json!("old"));
        state.shared_context_set("k", json!("new"));
        assert_eq!(state.shared_context_get("k"), Some(&json!("new")));
    }

    #[test]
    fn context_stores_complex_values() {
        let mut state = make_state();
        let val = json!({"nested": {"array": [1, 2, 3], "bool": true}});
        state.shared_context_set("complex", val.clone());
        assert_eq!(state.shared_context_get("complex"), Some(&val));
    }

    // ── Task board ────────────────────────────────────────────────────────────

    #[test]
    fn claim_task_succeeds_when_unclaimed() {
        let mut state = make_state();
        let result = state.claim_task("task-1", "Do the thing", 0);
        assert_eq!(result, ClaimResult::Claimed);
        assert!(state.task_board.contains_key("task-1"));
    }

    #[test]
    fn claim_task_fails_when_held_by_other() {
        let mut state = make_state();
        state.claim_task("task-1", "Do the thing", 0);
        let result = state.claim_task("task-1", "Do the thing", 1);
        match result {
            ClaimResult::AlreadyClaimed { held_by, .. } => assert_eq!(held_by, 0),
            _ => panic!("Expected AlreadyClaimed"),
        }
    }

    #[test]
    fn claim_task_is_idempotent_for_same_pane() {
        let mut state = make_state();
        state.claim_task("task-1", "original", 0);
        let result = state.claim_task("task-1", "re-claim", 0);
        assert_eq!(result, ClaimResult::Claimed);
        // Description updated.
        assert_eq!(state.task_board["task-1"].description, "re-claim");
    }

    #[test]
    fn release_task_succeeds_for_owner() {
        let mut state = make_state();
        state.claim_task("task-1", "desc", 0);
        let released = state.release_task("task-1", 0);
        assert!(released);
        assert!(!state.task_board.contains_key("task-1"));
    }

    #[test]
    fn release_task_fails_for_non_owner() {
        let mut state = make_state();
        state.claim_task("task-1", "desc", 0);
        let released = state.release_task("task-1", 1);
        assert!(!released);
        // Task still held by pane 0.
        assert!(state.task_board.contains_key("task-1"));
    }

    #[test]
    fn release_unclaimed_task_returns_false() {
        let mut state = make_state();
        assert!(!state.release_task("ghost-task", 0));
    }

    #[test]
    fn multiple_tasks_independent() {
        let mut state = make_state();
        state.claim_task("task-a", "A", 0);
        state.claim_task("task-b", "B", 1);
        // Pane 1 can't take task-a.
        assert!(matches!(
            state.claim_task("task-a", "", 1),
            ClaimResult::AlreadyClaimed { .. }
        ));
        // Pane 0 can't take task-b.
        assert!(matches!(
            state.claim_task("task-b", "", 0),
            ClaimResult::AlreadyClaimed { .. }
        ));
        // But pane 0 can release task-a.
        assert!(state.release_task("task-a", 0));
        // Now pane 1 can claim task-a.
        assert_eq!(state.claim_task("task-a", "", 1), ClaimResult::Claimed);
    }

    // ── MessagePriority serde ─────────────────────────────────────────────────

    #[test]
    fn message_priority_serializes_lowercase() {
        assert_eq!(
            serde_json::to_string(&MessagePriority::Normal).unwrap(),
            r#""normal""#
        );
        assert_eq!(
            serde_json::to_string(&MessagePriority::Urgent).unwrap(),
            r#""urgent""#
        );
    }

    #[test]
    fn message_priority_deserializes() {
        let n: MessagePriority = serde_json::from_str(r#""normal""#).unwrap();
        let u: MessagePriority = serde_json::from_str(r#""urgent""#).unwrap();
        assert_eq!(n, MessagePriority::Normal);
        assert_eq!(u, MessagePriority::Urgent);
    }

    // ── Pane registration & partner resolution ────────────────────────────────

    #[test]
    fn register_and_resolve_partner() {
        let mut state = make_state();
        state.register_pane(0);
        state.register_pane(1);
        assert_eq!(state.resolve_partner(0), Some(1));
        assert_eq!(state.resolve_partner(1), Some(0));
    }

    #[test]
    fn resolve_partner_none_when_alone() {
        let mut state = make_state();
        state.register_pane(5);
        assert_eq!(state.resolve_partner(5), None);
    }

    #[test]
    fn resolve_partner_works_with_non_sequential_ids() {
        let mut state = make_state();
        state.register_pane(3);
        state.register_pane(7);
        assert_eq!(state.resolve_partner(3), Some(7));
        assert_eq!(state.resolve_partner(7), Some(3));
    }

    #[test]
    fn unregister_pane_removes_from_known() {
        let mut state = make_state();
        state.register_pane(0);
        state.register_pane(1);
        state.unregister_pane(1);
        assert_eq!(state.resolve_partner(0), None);
        assert_eq!(state.known_panes, vec![0]);
    }

    #[test]
    fn unregister_pane_cleans_up_role_inbox_tasks() {
        let mut state = make_state();
        state.register_pane(0);
        state.register_pane(1);
        state.set_role(
            1,
            PaneRole {
                name: "tester".into(),
                description: "tests".into(),
            },
        );
        state.send_message(0, 1, "hello", MessagePriority::Normal);
        state.claim_task("t-1", "fix bug", 1);

        state.unregister_pane(1);

        assert!(state.roles.get(&1).is_none(), "role should be cleaned up");
        assert!(
            state.inboxes.get(&1).is_none(),
            "inbox should be cleaned up"
        );
        assert!(
            !state.task_board.contains_key("t-1"),
            "task claim should be released"
        );
    }

    #[test]
    fn register_pane_is_idempotent() {
        let mut state = make_state();
        state.register_pane(0);
        state.register_pane(0);
        state.register_pane(0);
        assert_eq!(state.known_panes.len(), 1);
    }
}
