//! Canonical event model for the Potato cockpit.
//!
//! All activity from agent adapters flows through [`AgentEvent`] variants.
//! The [`EventBus`] is a broadcast channel that fans events out to all subscribers.

use tokio::sync::broadcast;

// ── AgentEvent ────────────────────────────────────────────────────────────────

/// Every event that can be emitted by an agent adapter or the Potato runtime.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum AgentEvent {
    /// A partial text token was streamed from the agent.
    TextDelta { text: String },
    /// A complete assistant message was received.
    TextDone { full_text: String },
    /// The agent started a tool invocation.
    ToolStart {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    /// A tool invocation completed successfully.
    ToolDone {
        id: String,
        output: String,
        duration_ms: u64,
        success: bool,
    },
    /// A tool invocation failed.
    ToolError { id: String, error: String },
    /// The agent requires user approval before running a tool.
    ApprovalRequired {
        tool_id: String,
        tool_name: String,
        input: serde_json::Value,
    },
    /// The user approved or denied a pending tool invocation.
    ApprovalDecision { tool_id: String, approved: bool },
    /// A new agent turn is starting.
    TurnStart,
    /// The current agent turn completed.
    TurnDone { usage: Option<UsageInfo> },
    /// The agent process bound to a session id.
    SessionBound { agent_session_id: String },
    /// The agent process started.
    AgentStarted {
        adapter: String,
        working_dir: String,
        model: Option<String>,
    },
    /// The agent process exited.
    AgentExited { exit_code: Option<i32> },
    /// A non-fatal error occurred.
    Error { message: String },
    /// A warning occurred.
    Warning { message: String },
    /// A raw line that was not parsed into a structured event.
    Raw { payload: String },
}

// ── UsageInfo ─────────────────────────────────────────────────────────────────

/// Token usage and cost information from a completed turn.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UsageInfo {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cost_usd: Option<f64>,
}

// ── EventBus ──────────────────────────────────────────────────────────────────

/// Broadcast channel capacity.
const EVENT_BUS_CAPACITY: usize = 1024;

/// A cloneable handle to the broadcast event bus.
///
/// Clone the [`EventBus`] to subscribe additional receivers.
#[derive(Debug, Clone)]
pub struct EventBus {
    sender: broadcast::Sender<AgentEvent>,
}

impl EventBus {
    /// Create a new event bus with capacity [`EVENT_BUS_CAPACITY`].
    pub fn new() -> Self {
        let (sender, _) = broadcast::channel(EVENT_BUS_CAPACITY);
        Self { sender }
    }

    /// Publish an event to all subscribers.
    ///
    /// If there are no subscribers, the send is silently dropped.
    pub fn publish(&self, event: AgentEvent) {
        let _ = self.sender.send(event);
    }

    /// Subscribe to the event bus and receive a new [`broadcast::Receiver`].
    pub fn subscribe(&self) -> broadcast::Receiver<AgentEvent> {
        self.sender.subscribe()
    }

    /// Return the underlying [`broadcast::Sender`] for integration with
    /// tasks that need to publish events directly.
    pub fn sender(&self) -> broadcast::Sender<AgentEvent> {
        self.sender.clone()
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_bus_publish_and_receive() {
        let bus = EventBus::new();
        let mut rx = bus.subscribe();
        bus.publish(AgentEvent::TurnStart);
        let event = rx.try_recv().expect("should have received event");
        assert!(matches!(event, AgentEvent::TurnStart));
    }

    #[test]
    fn event_bus_multiple_subscribers() {
        let bus = EventBus::new();
        let mut rx1 = bus.subscribe();
        let mut rx2 = bus.subscribe();
        bus.publish(AgentEvent::Error { message: "oops".into() });
        assert!(rx1.try_recv().is_ok());
        assert!(rx2.try_recv().is_ok());
    }

    #[test]
    fn usage_info_optional_cost() {
        let info = UsageInfo { input_tokens: 10, output_tokens: 20, cost_usd: None };
        assert_eq!(info.input_tokens, 10);
        assert!(info.cost_usd.is_none());
    }

    #[test]
    fn agent_event_serialization_roundtrip() {
        let event = AgentEvent::TextDelta { text: "hello".into() };
        let json = serde_json::to_string(&event).unwrap();
        let decoded: AgentEvent = serde_json::from_str(&json).unwrap();
        if let AgentEvent::TextDelta { text } = decoded {
            assert_eq!(text, "hello");
        } else {
            panic!("unexpected variant");
        }
    }

    #[test]
    fn session_bound_event() {
        let event = AgentEvent::SessionBound { agent_session_id: "abc-123".into() };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("abc-123"));
    }
}
