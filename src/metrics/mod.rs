//! Session metrics — aggregates token usage, costs, tool call counts, and
//! timing information by subscribing to the event bus.

use std::time::Instant;

use tokio::sync::broadcast;
use tracing::debug;

use crate::events::AgentEvent;

// ── SessionMetrics ────────────────────────────────────────────────────────────

/// Aggregated metrics for a single agent session.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct SessionMetrics {
    /// Total input (prompt) tokens consumed.
    pub input_tokens: u64,
    /// Total output (completion) tokens generated.
    pub output_tokens: u64,
    /// Estimated total cost in USD.
    pub total_cost_usd: f64,
    /// Number of tool invocations started.
    pub tool_calls: u64,
    /// Number of tool invocations that ended with an error.
    pub tool_errors: u64,
    /// Elapsed wall-clock seconds since the session started.
    pub duration_secs: u64,
    /// Number of completed turns.
    pub turn_count: u64,
}

impl SessionMetrics {
    /// Return total tokens (input + output).
    #[must_use]
    pub fn total_tokens(&self) -> u64 {
        self.input_tokens + self.output_tokens
    }
}

// ── MetricsCollector ──────────────────────────────────────────────────────────

/// Subscribes to an event bus receiver and accumulates [`SessionMetrics`].
///
/// Call [`MetricsCollector::process`] in a loop (or spawn it as a task with
/// [`MetricsCollector::run`]) to keep metrics up-to-date.
pub struct MetricsCollector {
    metrics: SessionMetrics,
    start: Instant,
    rx: broadcast::Receiver<AgentEvent>,
}

impl MetricsCollector {
    /// Create a new collector subscribed to `rx`.
    pub fn new(rx: broadcast::Receiver<AgentEvent>) -> Self {
        Self {
            metrics: SessionMetrics::default(),
            start: Instant::now(),
            rx,
        }
    }

    /// Return a snapshot of the current metrics.
    #[must_use]
    pub fn snapshot(&self) -> SessionMetrics {
        let mut m = self.metrics.clone();
        m.duration_secs = self.start.elapsed().as_secs();
        m
    }

    /// Process a single event and update metrics.
    ///
    /// Returns `false` if the receiver is closed (no more events expected).
    pub async fn process(&mut self) -> bool {
        match self.rx.recv().await {
            Ok(event) => {
                self.handle_event(&event);
                true
            }
            Err(broadcast::error::RecvError::Lagged(n)) => {
                debug!("metrics collector lagged, skipped {} events", n);
                true
            }
            Err(broadcast::error::RecvError::Closed) => false,
        }
    }

    /// Drive the collector until the channel closes.
    ///
    /// Suitable for spawning as a background tokio task:
    /// ```ignore
    /// tokio::spawn(async move { collector.run().await });
    /// ```
    pub async fn run(mut self) {
        while self.process().await {}
        debug!("metrics collector finished");
    }

    // ── Internal ─────────────────────────────────────────────────────────────

    fn handle_event(&mut self, event: &AgentEvent) {
        match event {
            AgentEvent::TurnDone { usage } => {
                self.metrics.turn_count += 1;
                if let Some(u) = usage {
                    self.metrics.input_tokens += u.input_tokens;
                    self.metrics.output_tokens += u.output_tokens;
                    if let Some(cost) = u.cost_usd {
                        self.metrics.total_cost_usd += cost;
                    }
                }
            }
            AgentEvent::ToolStart { .. } => {
                self.metrics.tool_calls += 1;
            }
            AgentEvent::ToolError { .. } => {
                self.metrics.tool_errors += 1;
            }
            _ => {}
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::{EventBus, UsageInfo};

    fn make_collector() -> (EventBus, MetricsCollector) {
        let bus = EventBus::new();
        let rx = bus.subscribe();
        let collector = MetricsCollector::new(rx);
        (bus, collector)
    }

    #[tokio::test]
    async fn counts_turn_done() {
        let (bus, mut collector) = make_collector();

        bus.publish(AgentEvent::TurnDone {
            usage: Some(UsageInfo {
                input_tokens: 100,
                output_tokens: 50,
                cost_usd: Some(0.001),
            }),
        });
        collector.process().await;

        let snap = collector.snapshot();
        assert_eq!(snap.turn_count, 1);
        assert_eq!(snap.input_tokens, 100);
        assert_eq!(snap.output_tokens, 50);
        assert!((snap.total_cost_usd - 0.001).abs() < 1e-6);
    }

    #[tokio::test]
    async fn counts_tool_calls() {
        let (bus, mut collector) = make_collector();

        bus.publish(AgentEvent::ToolStart {
            id: "t1".into(),
            name: "read_file".into(),
            input: serde_json::json!({}),
        });
        bus.publish(AgentEvent::ToolStart {
            id: "t2".into(),
            name: "shell".into(),
            input: serde_json::json!({}),
        });
        bus.publish(AgentEvent::ToolError {
            id: "t1".into(),
            error: "oops".into(),
        });

        for _ in 0..3 {
            collector.process().await;
        }

        let snap = collector.snapshot();
        assert_eq!(snap.tool_calls, 2);
        assert_eq!(snap.tool_errors, 1);
    }

    #[tokio::test]
    async fn accumulates_multiple_turns() {
        let (bus, mut collector) = make_collector();

        for i in 0..3u64 {
            bus.publish(AgentEvent::TurnDone {
                usage: Some(UsageInfo {
                    input_tokens: 10 * i,
                    output_tokens: 5 * i,
                    cost_usd: None,
                }),
            });
        }
        for _ in 0..3 {
            collector.process().await;
        }

        let snap = collector.snapshot();
        assert_eq!(snap.turn_count, 3);
        assert_eq!(snap.input_tokens, 10 + 20);
        assert_eq!(snap.output_tokens, 5 + 10);
    }

    #[test]
    fn total_tokens_helper() {
        let m = SessionMetrics {
            input_tokens: 30,
            output_tokens: 20,
            ..Default::default()
        };
        assert_eq!(m.total_tokens(), 50);
    }

    #[test]
    fn snapshot_includes_duration() {
        let bus = EventBus::new();
        let rx = bus.subscribe();
        let collector = MetricsCollector::new(rx);
        let snap = collector.snapshot();
        // Duration should be 0 or very small just after creation.
        assert!(snap.duration_secs <= 1);
    }

    #[tokio::test]
    async fn turn_done_without_usage() {
        let (bus, mut collector) = make_collector();
        bus.publish(AgentEvent::TurnDone { usage: None });
        collector.process().await;
        let snap = collector.snapshot();
        assert_eq!(snap.turn_count, 1);
        assert_eq!(snap.input_tokens, 0);
    }
}
