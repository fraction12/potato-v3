//! Connection pool — manages multiple LLM client instances for load-balancing.

use std::sync::Arc;

use super::LlmClient;

/// A pool of LLM clients that can be selected round-robin or by preference.
#[derive(Default)]
pub struct ConnectionPool {
    clients: Vec<Arc<dyn LlmClient>>,
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
        self.clients.push(client);
    }

    /// Return the next client in round-robin order, or `None` if pool is empty.
    pub fn get(&mut self) -> Option<Arc<dyn LlmClient>> {
        if self.clients.is_empty() {
            return None;
        }
        let client = self.clients[self.next % self.clients.len()].clone();
        self.next += 1;
        Some(client)
    }

    /// Number of clients in the pool.
    pub fn len(&self) -> usize {
        self.clients.len()
    }

    /// Whether the pool is empty.
    pub fn is_empty(&self) -> bool {
        self.clients.is_empty()
    }
}
