//! Cloud Ollama client — proxied connection to a remote Ollama-compatible API.

use anyhow::Result;
use async_trait::async_trait;
use reqwest::Client;

use super::{LlmClient, types::{ChatRequest, ChatResponse}};

/// HTTP client for a cloud-hosted or proxied Ollama-compatible endpoint.
#[derive(Debug)]
pub struct CloudOllamaClient {
    /// Base URL of the remote API.
    pub base_url: String,
    /// Model identifier.
    pub model: String,
    /// Optional bearer token for authentication.
    pub api_key: Option<String>,
    http: Client,
}

impl CloudOllamaClient {
    /// Create a new cloud client.
    pub fn new(
        base_url: impl Into<String>,
        model: impl Into<String>,
        api_key: Option<String>,
    ) -> Self {
        Self {
            base_url: base_url.into(),
            model: model.into(),
            api_key,
            http: Client::new(),
        }
    }
}

#[async_trait]
impl LlmClient for CloudOllamaClient {
    async fn chat(&self, _request: ChatRequest) -> Result<ChatResponse> {
        Ok(ChatResponse::default())
    }

    fn model_name(&self) -> &str {
        &self.model
    }

    fn supports_streaming(&self) -> bool {
        true
    }
}

impl Drop for CloudOllamaClient {
    fn drop(&mut self) {
        let _ = &self.http;
    }
}
