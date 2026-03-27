//! Local Ollama client — connects to a locally running `ollama serve` instance.

use anyhow::{Context, Result};
use async_trait::async_trait;
use futures::StreamExt;
use reqwest::Client;
use tokio::sync::mpsc;
use tracing::{debug, warn};

use super::{
    LlmClient,
    types::{
        ChatRequest, ChatResponse, OllamaChatLine, OllamaTagsResponse, StreamChunk, ToolCall,
    },
};

/// HTTP client for a local `ollama serve` instance (default: `http://localhost:11434`).
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

    /// Create a client using the default local Ollama URL (`http://localhost:11434`).
    pub fn default_local(model: impl Into<String>) -> Self {
        Self::new("http://localhost:11434", model)
    }

    // ─── internal helpers ────────────────────────────────────────────────────

    /// Build the Ollama `/api/chat` body from a [`ChatRequest`].
    fn build_body(&self, request: &ChatRequest, stream: bool) -> serde_json::Value {
        serde_json::json!({
            "model": &self.model,
            "messages": &request.messages,
            "stream": stream,
            "options": {
                "temperature": request.temperature.unwrap_or(0.7),
                "num_predict": request.max_tokens.unwrap_or(4096),
            }
        })
    }

    /// Execute a single streaming request, accumulate chunks via `tx`.
    ///
    /// Returns a [`ChatResponse`] built from the accumulated stream.
    async fn stream_once(
        &self,
        request: &ChatRequest,
        tx: &mpsc::Sender<StreamChunk>,
    ) -> Result<ChatResponse> {
        let url = format!("{}/api/chat", self.base_url);
        let body = self.build_body(request, true);

        debug!("POST {url} model={}", self.model);

        let resp = self
            .http
            .post(&url)
            .json(&body)
            .send()
            .await
            .context("connecting to local Ollama")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            anyhow::bail!("Ollama returned {status}: {text}");
        }

        let mut byte_stream = resp.bytes_stream();
        let mut line_buf = String::new();

        // Assembled final response fields.
        let mut full_content = String::new();
        let mut all_tool_calls: Vec<ToolCall> = Vec::new();
        let mut prompt_tokens = 0u64;
        let mut completion_tokens = 0u64;

        while let Some(chunk) = byte_stream.next().await {
            let bytes = chunk.context("reading bytes from Ollama stream")?;
            let text = String::from_utf8_lossy(&bytes);

            for ch in text.chars() {
                if ch == '\n' {
                    let trimmed = line_buf.trim().to_string();
                    line_buf.clear();

                    if trimmed.is_empty() {
                        continue;
                    }

                    match serde_json::from_str::<OllamaChatLine>(&trimmed) {
                        Ok(line) => {
                            let stream_chunk = line.into_stream_chunk();

                            // Accumulate stats from the final chunk.
                            if stream_chunk.done {
                                prompt_tokens = stream_chunk.prompt_tokens.unwrap_or(0);
                                completion_tokens = stream_chunk.completion_tokens.unwrap_or(0);
                            }

                            // Collect full text and tool calls.
                            if !stream_chunk.content.is_empty() {
                                full_content.push_str(&stream_chunk.content);
                            }
                            all_tool_calls.extend(stream_chunk.tool_calls.clone());

                            // Forward chunk to caller.
                            if tx.send(stream_chunk).await.is_err() {
                                debug!("stream receiver dropped; stopping Ollama stream");
                                break;
                            }
                        }
                        Err(e) => {
                            warn!("failed to parse Ollama NDJSON line: {e} — line: {trimmed}");
                        }
                    }
                } else {
                    line_buf.push(ch);
                }
            }
        }

        // Flush any leftover partial line.
        let trimmed = line_buf.trim().to_string();
        if !trimmed.is_empty() {
            if let Ok(line) = serde_json::from_str::<OllamaChatLine>(&trimmed) {
                let sc = line.into_stream_chunk();
                full_content.push_str(&sc.content);
                all_tool_calls.extend(sc.tool_calls);
            }
        }

        use super::types::ChatMessage;
        let mut msg = ChatMessage::assistant(full_content);
        if !all_tool_calls.is_empty() {
            msg.tool_calls = Some(all_tool_calls);
        }

        Ok(ChatResponse {
            message: msg,
            prompt_tokens,
            completion_tokens,
            done: true,
        })
    }
}

#[async_trait]
impl LlmClient for LocalOllamaClient {
    /// Send a blocking (non-streaming) chat request to the local Ollama instance.
    async fn chat(&self, request: ChatRequest) -> Result<ChatResponse> {
        let url = format!("{}/api/chat", self.base_url);
        let body = self.build_body(&request, false);

        let resp = self
            .http
            .post(&url)
            .json(&body)
            .send()
            .await
            .context("connecting to local Ollama (non-stream)")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            anyhow::bail!("Ollama returned {status}: {text}");
        }

        // Ollama non-stream response mirrors the final NDJSON line shape.
        let line: OllamaChatLine = resp
            .json()
            .await
            .context("parsing Ollama chat response")?;

        let sc = line.into_stream_chunk();

        use super::types::ChatMessage;
        let mut msg = ChatMessage::assistant(sc.content);
        if !sc.tool_calls.is_empty() {
            msg.tool_calls = Some(sc.tool_calls);
        }

        Ok(ChatResponse {
            message: msg,
            prompt_tokens: sc.prompt_tokens.unwrap_or(0),
            completion_tokens: sc.completion_tokens.unwrap_or(0),
            done: true,
        })
    }

    fn model_name(&self) -> &str {
        &self.model
    }

    fn supports_streaming(&self) -> bool {
        true
    }

    /// Stream a chat request to the local Ollama instance.
    ///
    /// Retries once on a connection error before emitting failure.
    async fn chat_stream(
        &self,
        request: ChatRequest,
        tx: mpsc::Sender<StreamChunk>,
    ) -> Result<ChatResponse> {
        match self.stream_once(&request, &tx).await {
            Ok(resp) => Ok(resp),
            Err(e) => {
                warn!("Ollama stream attempt 1 failed: {e}. Retrying…");
                self.stream_once(&request, &tx)
                    .await
                    .context("Ollama stream failed after retry")
            }
        }
    }

    /// List all models available from the local Ollama instance via `GET /api/tags`.
    async fn list_models(&self) -> Result<Vec<String>> {
        let url = format!("{}/api/tags", self.base_url);
        let resp = self
            .http
            .get(&url)
            .send()
            .await
            .context("fetching Ollama model list")?;

        if !resp.status().is_success() {
            let status = resp.status();
            anyhow::bail!("Ollama /api/tags returned {status}");
        }

        let tags: OllamaTagsResponse = resp.json().await.context("parsing Ollama tags")?;
        Ok(tags.models.into_iter().map(|m| m.name).collect())
    }
}
