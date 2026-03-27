//! Legacy cloud Ollama client — stub without reqwest for legacy test compilation.

use anyhow::Result;
use async_trait::async_trait;

use super::{
    LlmClient,
    types::{ChatRequest, ChatResponse},
};

/// Stub for cloud OpenAI-compatible HTTP client (reqwest removed).
#[derive(Debug)]
pub struct CloudOllamaClient {
    pub base_url: String,
    pub model: String,
    pub api_key: Option<String>,
}

impl CloudOllamaClient {
    pub fn new(
        base_url: impl Into<String>,
        model: impl Into<String>,
        api_key: Option<String>,
    ) -> Self {
        Self { base_url: base_url.into(), model: model.into(), api_key }
    }
}

#[async_trait]
impl LlmClient for CloudOllamaClient {
    async fn chat(&self, _request: ChatRequest) -> Result<ChatResponse> {
        anyhow::bail!("CloudOllamaClient is a legacy stub — not usable without reqwest")
    }

    fn model_name(&self) -> &str {
        &self.model
    }
}
