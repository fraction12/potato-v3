//! Legacy LLM client abstraction — local Ollama and cloud-proxied variants.
//! Retired from main code path; preserved for test coverage.

pub mod cloud;
pub mod local;
pub mod pool;
pub mod types;

use anyhow::Result;
use async_trait::async_trait;
use tokio::sync::mpsc;

use types::{ChatRequest, ChatResponse, StreamChunk};

#[allow(unused_imports)]
pub use async_trait::async_trait as ollama_async_trait;

/// Shared interface for all LLM backends.
#[async_trait]
pub trait LlmClient: Send + Sync {
    async fn chat(&self, request: ChatRequest) -> Result<ChatResponse>;
    fn model_name(&self) -> &str;
    fn supports_streaming(&self) -> bool { false }

    async fn chat_stream(
        &self,
        request: ChatRequest,
        tx: mpsc::Sender<StreamChunk>,
    ) -> Result<ChatResponse> {
        let response = self.chat(request).await?;
        let _ = tx.send(StreamChunk {
            content: response.message.content.clone(),
            done: false,
            ..Default::default()
        }).await;
        let _ = tx.send(StreamChunk {
            content: String::new(),
            done: true,
            prompt_tokens: Some(response.prompt_tokens),
            completion_tokens: Some(response.completion_tokens),
            ..Default::default()
        }).await;
        Ok(response)
    }

    async fn list_models(&self) -> Result<Vec<String>> {
        Ok(vec![self.model_name().to_string()])
    }
}
