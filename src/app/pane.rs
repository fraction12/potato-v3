//! Pane management for multi-session cockpit layout.
//!
//! A [`Pane`] bundles one Claude session's state with its PTY and log tracker
//! handles. [`PaneManager`] owns up to [`MAX_PANES`] simultaneous panes and
//! tracks which one currently has input focus.

use crate::claude_log::ClaudeSessionLogTracker;
use crate::pty::RealPty;

use super::state::SessionState;

/// Hard maximum number of simultaneous panes (Claude, Codex, OpenCode).
pub const MAX_PANES: usize = 3;

/// A single session pane in the cockpit.
///
/// Owns the session state and optional runtime handles (PTY, log tracker).
/// The handles are `Option` because they are set up asynchronously after spawn.
#[derive(Debug)]
pub struct Pane {
    /// Unique pane id (monotonically increasing within a PaneManager lifetime).
    pub id: u64,
    /// The session state for this pane.
    pub session: SessionState,
    /// Live PTY wrapping the Claude process, if spawned.
    pub pty: Option<RealPty>,
    /// Claude JSONL log tracker for sidebar metrics.
    pub log: Option<ClaudeSessionLogTracker>,
    /// Number of events already persisted (for incremental JSONL reading).
    pub persisted_event_count: u64,
    /// Optional role name assigned to this pane (e.g. `"architect"`).
    ///
    /// Set via the `/role` command.  Broadcast as a notification to all other
    /// panes so they know this pane's current context.
    pub role_name: Option<String>,
    /// Optional free-text description accompanying the role.
    pub role_description: Option<String>,
}

impl Pane {
    /// Create a new pane with the given `id`, `session_id`, and `agent_name`.
    pub fn new(id: u64, session_id: impl Into<String>, agent_name: impl Into<String>) -> Self {
        Self {
            id,
            session: SessionState::new(session_id, agent_name),
            pty: None,
            log: None,
            persisted_event_count: 0,
            role_name: None,
            role_description: None,
        }
    }

    /// Returns true if the PTY is still alive (has not been dropped/taken).
    pub fn is_alive(&self) -> bool {
        self.pty.is_some()
    }
}

/// Manages up to [`MAX_PANES`] simultaneous session panes.
///
/// Invariants:
/// - `panes` length is always 0..=MAX_PANES.
/// - `active` is always a valid index into `panes` when `panes` is non-empty.
/// - When `panes` is empty, `active` is 0.
#[derive(Debug, Default)]
pub struct PaneManager {
    panes: Vec<Pane>,
    active: usize,
    next_id: u64,
}

impl PaneManager {
    /// Create an empty pane manager with no open panes.
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of open panes.
    pub fn len(&self) -> usize {
        self.panes.len()
    }

    /// Whether there are no open panes.
    pub fn is_empty(&self) -> bool {
        self.panes.is_empty()
    }

    /// Index of the active (focused) pane.
    pub fn active_index(&self) -> usize {
        self.active
    }

    /// The id that will be assigned to the next pane opened.
    pub fn next_id(&self) -> u64 {
        self.next_id
    }

    /// Whether there is room for another pane.
    pub fn can_open(&self) -> bool {
        self.panes.len() < MAX_PANES
    }

    /// Open a new pane. Returns `Some(&mut Pane)` on success, `None` if at capacity.
    pub fn open(
        &mut self,
        session_id: impl Into<String>,
        agent_name: impl Into<String>,
    ) -> Option<&mut Pane> {
        if !self.can_open() {
            return None;
        }
        let id = self.next_id;
        self.next_id += 1;
        let pane = Pane::new(id, session_id, agent_name);
        self.panes.push(pane);
        // New pane gets focus.
        self.active = self.panes.len() - 1;
        self.panes.last_mut()
    }

    /// Close pane at `index`. Returns the removed Pane (so caller can clean up PTY).
    /// Returns `None` if index is out of bounds.
    pub fn close(&mut self, index: usize) -> Option<Pane> {
        if index >= self.panes.len() {
            return None;
        }
        let pane = self.panes.remove(index);
        // Fix active index.
        if self.panes.is_empty() {
            self.active = 0;
        } else if self.active >= self.panes.len() {
            self.active = self.panes.len() - 1;
        } else if self.active > index {
            self.active = self.active.saturating_sub(1);
        }
        Some(pane)
    }

    /// Close the active pane. Returns the removed Pane.
    pub fn close_active(&mut self) -> Option<Pane> {
        if self.panes.is_empty() {
            return None;
        }
        self.close(self.active)
    }

    /// Switch focus to the next pane (wraps around).
    pub fn focus_next(&mut self) {
        if self.panes.len() > 1 {
            self.active = (self.active + 1) % self.panes.len();
        }
    }

    /// Switch focus to the previous pane (wraps around).
    pub fn focus_prev(&mut self) {
        if self.panes.len() > 1 {
            self.active = if self.active == 0 {
                self.panes.len() - 1
            } else {
                self.active - 1
            };
        }
    }

    /// Set focus to a specific pane index. No-op if out of bounds.
    pub fn focus(&mut self, index: usize) {
        if index < self.panes.len() {
            self.active = index;
        }
    }

    /// Reference to the active pane.
    pub fn active_pane(&self) -> Option<&Pane> {
        self.panes.get(self.active)
    }

    /// Mutable reference to the active pane.
    pub fn active_pane_mut(&mut self) -> Option<&mut Pane> {
        self.panes.get_mut(self.active)
    }

    /// Reference to pane at index.
    pub fn get(&self, index: usize) -> Option<&Pane> {
        self.panes.get(index)
    }

    /// Mutable reference to pane at index.
    pub fn get_mut(&mut self, index: usize) -> Option<&mut Pane> {
        self.panes.get_mut(index)
    }

    /// Iterate over all panes with their index.
    pub fn iter(&self) -> impl Iterator<Item = (usize, &Pane)> {
        self.panes.iter().enumerate()
    }

    /// Mutable iteration over all panes with their index.
    pub fn iter_mut(&mut self) -> impl Iterator<Item = (usize, &mut Pane)> {
        self.panes.iter_mut().enumerate()
    }

    /// Find the pane index whose session has the given claude_session_id.
    pub fn find_by_session_id(&self, session_id: &str) -> Option<usize> {
        self.panes
            .iter()
            .position(|p| p.session.claude_session_id.as_deref() == Some(session_id))
    }

    /// Find the pane index whose pane.id matches.
    pub fn find_by_pane_id(&self, pane_id: u64) -> Option<usize> {
        self.panes.iter().position(|p| p.id == pane_id)
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_manager_is_empty() {
        let pm = PaneManager::new();
        assert!(pm.is_empty());
        assert_eq!(pm.len(), 0);
        assert!(pm.can_open());
        assert!(pm.active_pane().is_none());
    }

    #[test]
    fn open_one_pane() {
        let mut pm = PaneManager::new();
        let pane = pm.open("sess-1", "claude").unwrap();
        assert_eq!(pane.session.session_id, "sess-1");
        assert_eq!(pane.id, 0);

        assert_eq!(pm.len(), 1);
        assert!(pm.can_open());
        assert_eq!(pm.active_index(), 0);
    }

    #[test]
    fn open_two_panes_focuses_second() {
        let mut pm = PaneManager::new();
        pm.open("sess-1", "claude");
        pm.open("sess-2", "claude");

        assert_eq!(pm.len(), 2);
        assert_eq!(pm.active_index(), 1);
        assert_eq!(pm.active_pane().unwrap().session.session_id, "sess-2");
    }

    #[test]
    fn open_at_capacity_returns_none() {
        let mut pm = PaneManager::new();
        for i in 0..MAX_PANES {
            pm.open(format!("sess-{}", i + 1), "claude");
        }
        assert_eq!(pm.len(), MAX_PANES);
        assert!(pm.open("sess-overflow", "claude").is_none());
        assert_eq!(pm.len(), MAX_PANES);
    }

    #[test]
    fn close_only_pane() {
        let mut pm = PaneManager::new();
        pm.open("sess-1", "claude");
        let closed = pm.close(0).unwrap();
        assert_eq!(closed.session.session_id, "sess-1");
        assert!(pm.is_empty());
        assert_eq!(pm.active_index(), 0);
    }

    #[test]
    fn close_active_when_first_of_two() {
        let mut pm = PaneManager::new();
        pm.open("sess-1", "claude");
        pm.open("sess-2", "claude");
        pm.focus(0);

        let closed = pm.close_active().unwrap();
        assert_eq!(closed.session.session_id, "sess-1");
        assert_eq!(pm.len(), 1);
        assert_eq!(pm.active_index(), 0);
        assert_eq!(pm.active_pane().unwrap().session.session_id, "sess-2");
    }

    #[test]
    fn close_second_pane_adjusts_active() {
        let mut pm = PaneManager::new();
        pm.open("sess-1", "claude");
        pm.open("sess-2", "claude");
        // active is 1 (second pane)

        let closed = pm.close(1).unwrap();
        assert_eq!(closed.session.session_id, "sess-2");
        assert_eq!(pm.len(), 1);
        assert_eq!(pm.active_index(), 0);
    }

    #[test]
    fn close_first_pane_when_active_is_second() {
        let mut pm = PaneManager::new();
        pm.open("sess-1", "claude");
        pm.open("sess-2", "claude");
        // active = 1

        pm.close(0);
        assert_eq!(pm.active_index(), 0);
        assert_eq!(pm.active_pane().unwrap().session.session_id, "sess-2");
    }

    #[test]
    fn focus_next_wraps() {
        let mut pm = PaneManager::new();
        pm.open("sess-1", "claude");
        pm.open("sess-2", "claude");
        pm.focus(0);

        pm.focus_next();
        assert_eq!(pm.active_index(), 1);
        pm.focus_next();
        assert_eq!(pm.active_index(), 0);
    }

    #[test]
    fn focus_prev_wraps() {
        let mut pm = PaneManager::new();
        pm.open("sess-1", "claude");
        pm.open("sess-2", "claude");
        pm.focus(0);

        pm.focus_prev();
        assert_eq!(pm.active_index(), 1);
        pm.focus_prev();
        assert_eq!(pm.active_index(), 0);
    }

    #[test]
    fn focus_noop_with_single_pane() {
        let mut pm = PaneManager::new();
        pm.open("sess-1", "claude");

        pm.focus_next();
        assert_eq!(pm.active_index(), 0);
        pm.focus_prev();
        assert_eq!(pm.active_index(), 0);
    }

    #[test]
    fn find_by_session_id() {
        let mut pm = PaneManager::new();
        pm.open("sess-1", "claude");
        pm.open("sess-2", "claude");
        pm.get_mut(0).unwrap().session.claude_session_id = Some("claude-abc".into());
        pm.get_mut(1).unwrap().session.claude_session_id = Some("claude-xyz".into());

        assert_eq!(pm.find_by_session_id("claude-abc"), Some(0));
        assert_eq!(pm.find_by_session_id("claude-xyz"), Some(1));
        assert_eq!(pm.find_by_session_id("nope"), None);
    }

    #[test]
    fn find_by_pane_id() {
        let mut pm = PaneManager::new();
        pm.open("sess-1", "claude");
        pm.open("sess-2", "claude");
        // ids are 0, 1
        assert_eq!(pm.find_by_pane_id(0), Some(0));
        assert_eq!(pm.find_by_pane_id(1), Some(1));
        assert_eq!(pm.find_by_pane_id(99), None);
    }

    #[test]
    fn pane_ids_are_unique_after_close_reopen() {
        let mut pm = PaneManager::new();
        pm.open("sess-1", "claude"); // id 0
        pm.close(0);
        pm.open("sess-2", "claude"); // id 1, not 0
        assert_eq!(pm.get(0).unwrap().id, 1);
    }

    #[test]
    fn close_out_of_bounds_returns_none() {
        let mut pm = PaneManager::new();
        assert!(pm.close(0).is_none());
        pm.open("sess-1", "claude");
        assert!(pm.close(5).is_none());
    }

    #[test]
    fn iter_yields_all_panes() {
        let mut pm = PaneManager::new();
        pm.open("sess-1", "claude");
        pm.open("sess-2", "claude");
        let ids: Vec<_> = pm
            .iter()
            .map(|(i, p)| (i, p.session.session_id.clone()))
            .collect();
        assert_eq!(ids, vec![(0, "sess-1".into()), (1, "sess-2".into())]);
    }

    #[test]
    fn pane_role_fields_default_to_none() {
        let mut pm = PaneManager::new();
        pm.open("sess-1", "claude");
        let pane = pm.active_pane().unwrap();
        assert!(pane.role_name.is_none());
        assert!(pane.role_description.is_none());
    }

    #[test]
    fn pane_role_can_be_set() {
        let mut pm = PaneManager::new();
        pm.open("sess-1", "claude");
        let pane = pm.active_pane_mut().unwrap();
        pane.role_name = Some("architect".into());
        pane.role_description = Some("Frontend API design".into());

        let pane = pm.active_pane().unwrap();
        assert_eq!(pane.role_name.as_deref(), Some("architect"));
        assert_eq!(
            pane.role_description.as_deref(),
            Some("Frontend API design")
        );
    }

    #[test]
    fn pane_role_cleared_on_close_reopen() {
        let mut pm = PaneManager::new();
        pm.open("sess-1", "claude");
        pm.active_pane_mut().unwrap().role_name = Some("architect".into());
        pm.close(0);

        pm.open("sess-2", "claude");
        // New pane should have no role.
        assert!(pm.active_pane().unwrap().role_name.is_none());
    }
}
