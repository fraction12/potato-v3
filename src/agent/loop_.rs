//! Agent loop — the async task that drives the AI ↔ tool execution cycle.

use tokio::sync::mpsc;

use crate::app::message::{AgentEvent, Message};

/// Run the agent loop, sending [`Message::Agent`] events through `tx`.
///
/// This is spawned as a background [`tokio::task`] and drives the full
/// prompt → stream → tool-call → approve → execute → repeat cycle.
pub async fn agent_loop(
    tx: mpsc::Sender<Message>,
    _model: String,
    _initial_prompt: Option<String>,
) {
    // Stub: immediately signal idle completion.
    let _ = tx.send(Message::Agent(AgentEvent::ResponseComplete)).await;
}
