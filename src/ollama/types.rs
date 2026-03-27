//! Shared data types for LLM requests and responses.

use serde::{Deserialize, Serialize};

/// A single message in a chat conversation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    /// Role: "system", "user", or "assistant".
    pub role: String,
    /// The message content.
    pub content: String,
}

impl ChatMessage {
    pub fn user(content: impl Into<String>) -> Self {
        Self { role: "user".into(), content: content.into() }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self { role: "assistant".into(), content: content.into() }
    }

    pub fn system(content: impl Into<String>) -> Self {
        Self { role: "system".into(), content: content.into() }
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
    /// The assistant's reply.
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

/// A single streamed chunk from the LLM.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamChunk {
    /// Partial content token.
    pub content: String,
    /// Whether this is the final chunk.
    pub done: bool,
    /// Tokens used (only populated in the final chunk).
    pub prompt_tokens: Option<u64>,
    pub completion_tokens: Option<u64>,
}
