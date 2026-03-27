//! LLM client abstraction — local Ollama and cloud-proxied variants.

pub mod cloud;
pub mod local;
pub mod pool;
pub mod types;

use anyhow::Result;
use async_trait::async_trait;

use types::{ChatRequest, ChatResponse};

// Re-export async_trait so implementors don't need it directly.
#[allow(unused_imports)]
pub use async_trait::async_trait as ollama_async_trait;

/// Shared interface for all LLM backends.
#[async_trait]
pub trait LlmClient: Send + Sync {
    /// Send a chat request and return a complete response.
    async fn chat(&self, request: ChatRequest) -> Result<ChatResponse>;

    /// Return the name/id of the model this client targets.
    fn model_name(&self) -> &str;

    /// Whether this client supports streaming.
    fn supports_streaming(&self) -> bool {
        false
    }
}
