//! Connection pool — manages multiple LLM client instances for load-balancing and health checks.

use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use tracing::{debug, warn};

use super::{
    LlmClient,
    types::{ChatMessage, ChatRequest},
};

/// Health status of a pooled client.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClientHealth {
    /// Client is reachable.
    Healthy,
    /// Client is known to be unreachable.
    Unhealthy,
    /// Not yet checked.
    Unknown,
}

/// A pooled entry wrapping an [`LlmClient`] with health metadata.
struct PoolEntry {
    client: Arc<dyn LlmClient>,
    health: ClientHealth,
    last_checked: Option<Instant>,
}

/// A pool of LLM clients that can be selected round-robin or by preference.
///
/// Unhealthy clients are skipped during selection.
#[derive(Default)]
pub struct ConnectionPool {
    entries: Vec<PoolEntry>,
    /// Index of the next client to use (round-robin).
    next: usize,
}

impl ConnectionPool {
    /// Create an empty pool.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a client to the pool.
    pub fn add(&mut self, client: Arc<dyn LlmClient>) {
        self.entries.push(PoolEntry {
            client,
            health: ClientHealth::Unknown,
            last_checked: None,
        });
    }

    /// Return the next **healthy** (or unknown) client in round-robin order,
    /// or `None` if the pool is empty or all clients are unhealthy.
    pub fn get(&mut self) -> Option<Arc<dyn LlmClient>> {
        if self.entries.is_empty() {
            return None;
        }

        let len = self.entries.len();
        for _ in 0..len {
            let idx = self.next % len;
            self.next += 1;
            let entry = &self.entries[idx];
            if entry.health != ClientHealth::Unhealthy {
                return Some(entry.client.clone());
            }
        }

        // All unhealthy — return first anyway as a last resort.
        warn!("All LLM clients are unhealthy; using first as fallback");
        Some(self.entries[0].client.clone())
    }

    /// Mark a client by model name as healthy or unhealthy.
    pub fn set_health(&mut self, model_name: &str, health: ClientHealth) {
        for entry in &mut self.entries {
            if entry.client.model_name() == model_name {
                entry.health = health.clone();
                entry.last_checked = Some(Instant::now());
            }
        }
    }

    /// Run a lightweight health-check ping against all clients whose status is
    /// [`ClientHealth::Unknown`] or whose last check was more than `stale_secs` ago.
    ///
    /// A health check sends a minimal single-token request and marks the client
    /// healthy/unhealthy based on whether the call succeeds.
    pub async fn health_check_all(&mut self, stale_secs: u64) {
        let stale = Duration::from_secs(stale_secs);
        let now = Instant::now();

        // Collect clients that need checking (can't borrow mutably inside async easily,
        // so we clone Arc first).
        let to_check: Vec<(usize, Arc<dyn LlmClient>)> = self
            .entries
            .iter()
            .enumerate()
            .filter(|(_, e)| {
                e.health == ClientHealth::Unknown
                    || e.last_checked.map_or(true, |t| now.duration_since(t) > stale)
            })
            .map(|(i, e)| (i, e.client.clone()))
            .collect();

        for (idx, client) in to_check {
            let healthy = ping_client(client.as_ref()).await;
            self.entries[idx].health = if healthy {
                ClientHealth::Healthy
            } else {
                ClientHealth::Unhealthy
            };
            self.entries[idx].last_checked = Some(Instant::now());
            debug!(
                model = client.model_name(),
                healthy,
                "health check complete"
            );
        }
    }

    /// Number of clients in the pool.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the pool is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Return health status for all clients.
    pub fn health_report(&self) -> Vec<(&str, &ClientHealth)> {
        self.entries
            .iter()
            .map(|e| (e.client.model_name(), &e.health))
            .collect()
    }
}

/// Send a single-token ping request to check whether the client is reachable.
async fn ping_client(client: &dyn LlmClient) -> bool {
    let req = ChatRequest {
        model: client.model_name().to_string(),
        messages: vec![ChatMessage::user("ping")],
        stream: false,
        max_tokens: Some(1),
        temperature: Some(0.0),
    };
    match client.chat(req).await {
        Ok(_) => true,
        Err(e) => {
            warn!(model = client.model_name(), error = %e, "health ping failed");
            false
        }
    }
}

// Implement Default for PoolEntry manually to satisfy the derive.
impl Default for PoolEntry {
    fn default() -> Self {
        unreachable!("PoolEntry should not be default-constructed")
    }
}
