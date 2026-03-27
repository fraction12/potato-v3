//! Shared data types for LLM requests and responses.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A single message in a chat conversation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    /// Role: "system", "user", "assistant", or "tool".
    pub role: String,
    /// The message content.
    pub content: String,
    /// Optional tool calls emitted by the assistant.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
}

impl ChatMessage {
    /// Construct a user-role message.
    pub fn user(content: impl Into<String>) -> Self {
        Self { role: "user".into(), content: content.into(), tool_calls: None }
    }

    /// Construct an assistant-role message.
    pub fn assistant(content: impl Into<String>) -> Self {
        Self { role: "assistant".into(), content: content.into(), tool_calls: None }
    }

    /// Construct a system-role message.
    pub fn system(content: impl Into<String>) -> Self {
        Self { role: "system".into(), content: content.into(), tool_calls: None }
    }

    /// Construct a tool-result message fed back into the conversation.
    pub fn tool_result(_tool_name: impl Into<String>, output: impl Into<String>) -> Self {
        Self {
            role: "tool".into(),
            content: output.into(),
            tool_calls: None,
        }
    }
}

/// A request payload sent to the LLM.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatRequest {
    /// Target model identifier.
    pub model: String,
    /// Conversation history including the new user turn.
    pub messages: Vec<ChatMessage>,
    /// Whether to request a streaming response.
    #[serde(default)]
    pub stream: bool,
    /// Optional max tokens cap.
    pub max_tokens: Option<u32>,
    /// Optional temperature (0.0 – 2.0).
    pub temperature: Option<f32>,
}

/// A complete (non-streaming) response from the LLM.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatResponse {
    /// The assistant's reply (may be empty when tool calls are present).
    pub message: ChatMessage,
    /// Total prompt tokens used.
    pub prompt_tokens: u64,
    /// Total completion tokens used.
    pub completion_tokens: u64,
    /// Whether generation finished normally.
    pub done: bool,
}

impl Default for ChatResponse {
    fn default() -> Self {
        Self {
            message: ChatMessage::assistant(""),
            prompt_tokens: 0,
            completion_tokens: 0,
            done: true,
        }
    }
}

// ──────────────────────────────────────────────
// Tool-call types (Ollama & OpenAI compatible)
// ──────────────────────────────────────────────

/// A tool call emitted by the model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    /// Optional id used by some APIs (e.g. OpenAI).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// The function to invoke.
    pub function: FunctionCall,
}

/// The function name and arguments within a [`ToolCall`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionCall {
    /// Name of the function / tool to call.
    pub name: String,
    /// Arguments as a JSON value (Ollama sends an object; OpenAI sends a string).
    #[serde(deserialize_with = "deserialize_arguments")]
    pub arguments: Value,
}

/// Deserialize arguments that may arrive as a JSON object **or** a JSON-encoded string.
fn deserialize_arguments<'de, D>(deserializer: D) -> Result<Value, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let v = Value::deserialize(deserializer)?;
    match v {
        // Ollama sends an object directly.
        Value::Object(_) | Value::Null => Ok(v),
        // OpenAI sends a stringified JSON object.
        Value::String(s) => {
            serde_json::from_str(&s).map_err(serde::de::Error::custom)
        }
        other => Ok(other),
    }
}

// ──────────────────────────────────────────────
// Streaming types
// ──────────────────────────────────────────────

/// A single streamed chunk from the LLM.
///
/// Handles both content-token chunks and tool-call chunks from Ollama / OpenAI SSE.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StreamChunk {
    /// Partial content token (may be empty when `tool_calls` is present).
    pub content: String,
    /// Tool calls requested by the model (populated instead of content).
    #[serde(default)]
    pub tool_calls: Vec<ToolCall>,
    /// Whether this is the final chunk.
    pub done: bool,
    /// Prompt tokens used (only populated in the final chunk).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_tokens: Option<u64>,
    /// Completion tokens used (only populated in the final chunk).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completion_tokens: Option<u64>,
}

// ──────────────────────────────────────────────
// Ollama wire types (internal deserialization)
// ──────────────────────────────────────────────

/// Raw NDJSON line returned by `POST /api/chat` (Ollama local format).
#[derive(Debug, Deserialize)]
pub(crate) struct OllamaChatLine {
    pub message: Option<OllamaMessage>,
    #[serde(default)]
    pub done: bool,
    pub prompt_eval_count: Option<u64>,
    pub eval_count: Option<u64>,
}

/// The `message` object inside an Ollama streaming line.
#[derive(Debug, Deserialize)]
pub(crate) struct OllamaMessage {
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub tool_calls: Vec<ToolCall>,
}

impl OllamaChatLine {
    /// Convert a raw Ollama line into the canonical [`StreamChunk`].
    pub(crate) fn into_stream_chunk(self) -> StreamChunk {
        let (content, tool_calls) = match self.message {
            Some(m) => (m.content, m.tool_calls),
            None => (String::new(), vec![]),
        };
        StreamChunk {
            content,
            tool_calls,
            done: self.done,
            prompt_tokens: self.prompt_eval_count,
            completion_tokens: self.eval_count,
        }
    }
}

// ──────────────────────────────────────────────
// OpenAI SSE wire types (internal deserialization)
// ──────────────────────────────────────────────

/// A `data: {...}` line from an OpenAI-compatible SSE stream.
#[derive(Debug, Deserialize)]
pub(crate) struct OpenAiSseLine {
    pub choices: Vec<OpenAiChoice>,
    pub usage: Option<OpenAiUsage>,
}

/// One choice delta inside an SSE line.
#[derive(Debug, Deserialize)]
pub(crate) struct OpenAiChoice {
    pub delta: OpenAiDelta,
    pub finish_reason: Option<String>,
}

/// The incremental content inside a choice.
#[derive(Debug, Deserialize, Default)]
pub(crate) struct OpenAiDelta {
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default)]
    pub tool_calls: Option<Vec<OpenAiToolCallDelta>>,
}

/// An incremental tool call chunk (OpenAI sends these piecemeal).
#[derive(Debug, Deserialize)]
pub(crate) struct OpenAiToolCallDelta {
    pub index: usize,
    pub id: Option<String>,
    pub function: Option<OpenAiFunctionDelta>,
}

/// Partial function data within a streaming tool call.
#[derive(Debug, Deserialize)]
pub(crate) struct OpenAiFunctionDelta {
    pub name: Option<String>,
    #[serde(default)]
    pub arguments: Option<String>,
}

/// Token usage reported in the final SSE chunk.
#[derive(Debug, Deserialize)]
pub(crate) struct OpenAiUsage {
    pub prompt_tokens: Option<u64>,
    pub completion_tokens: Option<u64>,
}

/// Ollama `GET /api/tags` response.
#[derive(Debug, Deserialize)]
pub(crate) struct OllamaTagsResponse {
    pub models: Vec<OllamaModelEntry>,
}

/// A single model entry from `GET /api/tags`.
#[derive(Debug, Deserialize)]
pub(crate) struct OllamaModelEntry {
    pub name: String,
}
