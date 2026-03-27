//! Cloud Ollama client — OpenAI-compatible `/v1/chat/completions` with SSE streaming.

use anyhow::{Context, Result};
use async_trait::async_trait;
use futures::StreamExt;
use reqwest::Client;
use tokio::sync::mpsc;
use tracing::{debug, warn};

use super::{
    LlmClient,
    types::{
        ChatRequest, ChatResponse, ChatMessage, FunctionCall, OpenAiSseLine, StreamChunk,
        ToolCall,
    },
};

/// HTTP client for a cloud-hosted or proxied OpenAI-compatible endpoint.
#[derive(Debug)]
pub struct CloudOllamaClient {
    /// Base URL of the remote API (e.g. `https://api.openai.com`).
    pub base_url: String,
    /// Model identifier (e.g. `gpt-4o`).
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

    // ─── internal helpers ────────────────────────────────────────────────────

    /// Build the OpenAI-compatible request body.
    fn build_body(&self, request: &ChatRequest, stream: bool) -> serde_json::Value {
        let mut body = serde_json::json!({
            "model": &self.model,
            "messages": &request.messages,
            "stream": stream,
        });
        if let Some(max_tokens) = request.max_tokens {
            body["max_tokens"] = serde_json::json!(max_tokens);
        }
        if let Some(temp) = request.temperature {
            body["temperature"] = serde_json::json!(temp);
        }
        body
    }

    /// Attach the Authorization header if an API key is configured.
    fn auth_header(&self) -> Option<String> {
        self.api_key
            .as_deref()
            .map(|k| format!("Bearer {k}"))
    }

    /// Execute a streaming request and forward chunks via `tx`.
    async fn stream_once(
        &self,
        request: &ChatRequest,
        tx: &mpsc::Sender<StreamChunk>,
    ) -> Result<ChatResponse> {
        let url = format!("{}/v1/chat/completions", self.base_url);
        let body = self.build_body(request, true);

        debug!("POST {url} model={}", self.model);

        let mut req = self.http.post(&url).json(&body);
        if let Some(auth) = self.auth_header() {
            req = req.header("Authorization", auth);
        }

        let resp = req.send().await.context("connecting to cloud LLM")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            anyhow::bail!("cloud LLM returned {status}: {text}");
        }

        let mut byte_stream = resp.bytes_stream();
        let mut line_buf = String::new();

        // Accumulate the full response.
        let mut full_content = String::new();
        // Tool call accumulator: index → (id, name, accumulated_args)
        let mut tool_acc: std::collections::HashMap<usize, (Option<String>, String, String)> =
            std::collections::HashMap::new();

        let mut prompt_tokens = 0u64;
        let mut completion_tokens = 0u64;
        let mut done = false;

        while let Some(chunk) = byte_stream.next().await {
            let bytes = chunk.context("reading bytes from cloud SSE stream")?;
            let text = String::from_utf8_lossy(&bytes);

            for ch in text.chars() {
                if ch == '\n' {
                    let trimmed = line_buf.trim().to_string();
                    line_buf.clear();

                    // SSE lines are prefixed with "data: ".
                    let data = if let Some(s) = trimmed.strip_prefix("data: ") {
                        s
                    } else {
                        continue;
                    };

                    if data == "[DONE]" {
                        done = true;
                        let _ = tx
                            .send(StreamChunk {
                                done: true,
                                prompt_tokens: Some(prompt_tokens),
                                completion_tokens: Some(completion_tokens),
                                ..Default::default()
                            })
                            .await;
                        break;
                    }

                    match serde_json::from_str::<OpenAiSseLine>(data) {
                        Ok(line) => {
                            // Collect token usage from any line that carries it.
                            if let Some(usage) = &line.usage {
                                prompt_tokens = usage.prompt_tokens.unwrap_or(prompt_tokens);
                                completion_tokens =
                                    usage.completion_tokens.unwrap_or(completion_tokens);
                            }

                            for choice in &line.choices {
                                // Content delta.
                                if let Some(content) = &choice.delta.content {
                                    if !content.is_empty() {
                                        full_content.push_str(content);
                                        let _ = tx
                                            .send(StreamChunk {
                                                content: content.clone(),
                                                ..Default::default()
                                            })
                                            .await;
                                    }
                                }

                                // Tool call delta — accumulate by index.
                                if let Some(tc_deltas) = &choice.delta.tool_calls {
                                    for tc in tc_deltas {
                                        let entry = tool_acc
                                            .entry(tc.index)
                                            .or_insert_with(|| (None, String::new(), String::new()));
                                        if let Some(id) = &tc.id {
                                            entry.0 = Some(id.clone());
                                        }
                                        if let Some(fn_delta) = &tc.function {
                                            if let Some(name) = &fn_delta.name {
                                                entry.1.push_str(name);
                                            }
                                            if let Some(args) = &fn_delta.arguments {
                                                entry.2.push_str(args);
                                            }
                                        }
                                    }
                                }

                                // finish_reason == "tool_calls" → emit accumulated tool calls.
                                if choice.finish_reason.as_deref() == Some("tool_calls") {
                                    let calls = assemble_tool_calls(&tool_acc);
                                    if !calls.is_empty() {
                                        let _ = tx
                                            .send(StreamChunk {
                                                tool_calls: calls.clone(),
                                                ..Default::default()
                                            })
                                            .await;
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            warn!("failed to parse SSE line: {e} — data: {data}");
                        }
                    }
                } else {
                    line_buf.push(ch);
                }
            }

            if done {
                break;
            }
        }

        // Build final tool call list for the response.
        let all_tool_calls = assemble_tool_calls(&tool_acc);
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

/// Assemble complete [`ToolCall`]s from accumulated deltas.
fn assemble_tool_calls(
    acc: &std::collections::HashMap<usize, (Option<String>, String, String)>,
) -> Vec<ToolCall> {
    let mut entries: Vec<(usize, &(Option<String>, String, String))> =
        acc.iter().map(|(k, v)| (*k, v)).collect();
    entries.sort_by_key(|(idx, _)| *idx);

    entries
        .into_iter()
        .filter_map(|(_, (id, name, args_str))| {
            if name.is_empty() {
                return None;
            }
            let arguments = serde_json::from_str(args_str)
                .unwrap_or(serde_json::Value::Object(serde_json::Map::new()));
            Some(ToolCall {
                id: id.clone(),
                function: FunctionCall { name: name.clone(), arguments },
            })
        })
        .collect()
}

#[async_trait]
impl LlmClient for CloudOllamaClient {
    /// Send a non-streaming chat request to the cloud endpoint.
    async fn chat(&self, request: ChatRequest) -> Result<ChatResponse> {
        let url = format!("{}/v1/chat/completions", self.base_url);
        let body = self.build_body(&request, false);

        let mut req = self.http.post(&url).json(&body);
        if let Some(auth) = self.auth_header() {
            req = req.header("Authorization", auth);
        }

        let resp = req.send().await.context("connecting to cloud LLM")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            anyhow::bail!("cloud LLM returned {status}: {text}");
        }

        // Parse the full JSON response.
        let val: serde_json::Value = resp.json().await.context("parsing cloud LLM response")?;

        let content = val["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or("")
            .to_string();

        let prompt_tokens = val["usage"]["prompt_tokens"].as_u64().unwrap_or(0);
        let completion_tokens = val["usage"]["completion_tokens"].as_u64().unwrap_or(0);

        // Extract tool calls if present.
        let tool_calls: Vec<ToolCall> =
            if let Some(tc_arr) = val["choices"][0]["message"]["tool_calls"].as_array() {
                tc_arr
                    .iter()
                    .filter_map(|tc| serde_json::from_value(tc.clone()).ok())
                    .collect()
            } else {
                vec![]
            };

        let mut msg = ChatMessage::assistant(content);
        if !tool_calls.is_empty() {
            msg.tool_calls = Some(tool_calls);
        }

        Ok(ChatResponse {
            message: msg,
            prompt_tokens,
            completion_tokens,
            done: true,
        })
    }

    fn model_name(&self) -> &str {
        &self.model
    }

    fn supports_streaming(&self) -> bool {
        true
    }

    /// Stream a chat request to the cloud endpoint via SSE.
    async fn chat_stream(
        &self,
        request: ChatRequest,
        tx: mpsc::Sender<StreamChunk>,
    ) -> Result<ChatResponse> {
        self.stream_once(&request, &tx)
            .await
            .context("cloud LLM streaming failed")
    }
}
