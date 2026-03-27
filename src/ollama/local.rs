//! Local Ollama client — connects to a locally running Ollama instance.

use anyhow::Result;
use async_trait::async_trait;
use reqwest::Client;

use super::{LlmClient, types::{ChatRequest, ChatResponse}};

/// HTTP client for a local `ollama serve` instance (default: localhost:11434).
#[derive(Debug)]
pub struct LocalOllamaClient {
    /// Base URL of the Ollama API.
    pub base_url: String,
    /// Model to use for chat requests.
    pub model: String,
    http: Client,
}

impl LocalOllamaClient {
    /// Create a new client pointing at `base_url` with the given model.
    pub fn new(base_url: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            model: model.into(),
            http: Client::new(),
        }
    }

    /// Create a client using the default local Ollama URL.
    pub fn default_local(model: impl Into<String>) -> Self {
        Self::new("http://localhost:11434", model)
    }
}

#[async_trait]
impl LlmClient for LocalOllamaClient {
    async fn chat(&self, _request: ChatRequest) -> Result<ChatResponse> {
        // Stub: returns an empty response.
        Ok(ChatResponse::default())
    }

    fn model_name(&self) -> &str {
        &self.model
    }

    fn supports_streaming(&self) -> bool {
        true
    }
}

// Suppress unused field warning on `http` until it's used.
impl Drop for LocalOllamaClient {
    fn drop(&mut self) {
        let _ = &self.http;
    }
}
