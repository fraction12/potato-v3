//! LLM client abstraction — local Ollama and cloud-proxied variants.

pub mod cloud;
pub mod local;
pub mod pool;
pub mod types;

use anyhow::Result;
use async_trait::async_trait;
use tokio::sync::mpsc;

use types::{ChatRequest, ChatResponse, StreamChunk};

// Re-export async_trait so implementors don't need it directly.
#[allow(unused_imports)]
pub use async_trait::async_trait as ollama_async_trait;

/// Shared interface for all LLM backends.
#[async_trait]
pub trait LlmClient: Send + Sync {
    /// Send a chat request and return a complete (non-streaming) response.
    async fn chat(&self, request: ChatRequest) -> Result<ChatResponse>;

    /// Return the name/id of the model this client targets.
    fn model_name(&self) -> &str;

    /// Whether this client supports native streaming.
    fn supports_streaming(&self) -> bool {
        false
    }

    /// Send a chat request and forward [`StreamChunk`]s via `tx`.
    ///
    /// Returns a [`ChatResponse`] assembled from the stream once generation
    /// is complete. The default implementation falls back to the non-streaming
    /// [`Self::chat`] method and emits a single chunk with the full response.
    async fn chat_stream(
        &self,
        request: ChatRequest,
        tx: mpsc::Sender<StreamChunk>,
    ) -> Result<ChatResponse> {
        let response = self.chat(request).await?;

        // Emit the full content as one token chunk followed by the done chunk.
        let _ = tx
            .send(StreamChunk {
                content: response.message.content.clone(),
                done: false,
                ..Default::default()
            })
            .await;

        let _ = tx
            .send(StreamChunk {
                content: String::new(),
                done: true,
                prompt_tokens: Some(response.prompt_tokens),
                completion_tokens: Some(response.completion_tokens),
                ..Default::default()
            })
            .await;

        Ok(response)
    }

    /// List available models from this backend.
    ///
    /// The default implementation returns only the client's configured model.
    async fn list_models(&self) -> Result<Vec<String>> {
        Ok(vec![self.model_name().to_string()])
    }
}
