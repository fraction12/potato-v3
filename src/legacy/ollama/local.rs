//! Legacy local Ollama client — stub without reqwest for legacy test compilation.

use anyhow::Result;
use async_trait::async_trait;
use tokio::sync::mpsc;

use super::{
    LlmClient,
    types::{ChatMessage, ChatRequest, ChatResponse, StreamChunk},
};

/// Stub for local Ollama HTTP client (reqwest removed).
#[derive(Debug)]
pub struct LocalOllamaClient {
    pub base_url: String,
    pub model: String,
}

impl LocalOllamaClient {
    pub fn new(base_url: impl Into<String>, model: impl Into<String>) -> Self {
        Self { base_url: base_url.into(), model: model.into() }
    }

    pub fn default_local(model: impl Into<String>) -> Self {
        Self::new("http://localhost:11434", model)
    }
}

#[async_trait]
impl LlmClient for LocalOllamaClient {
    async fn chat(&self, _request: ChatRequest) -> Result<ChatResponse> {
        anyhow::bail!("LocalOllamaClient is a legacy stub — not usable without reqwest")
    }

    fn model_name(&self) -> &str {
        &self.model
    }
}
