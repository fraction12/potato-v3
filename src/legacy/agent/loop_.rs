//! Legacy agent loop — retired from main code path; preserved for test coverage.

use std::sync::Arc;

use anyhow::Result;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

use crate::app::message::{AgentCommand, AgentEvent, Message};
use crate::legacy::ollama::{
    LlmClient,
    types::{ChatMessage, ChatRequest, StreamChunk},
};
use crate::legacy::tools::registry::ToolRegistry;

use super::{
    state_machine::AgentState,
    streaming::StreamAccumulator,
};

/// Maximum number of consecutive tool-call rounds before the loop bails out.
const MAX_TOOL_ROUNDS: usize = 16;

/// Run the agent loop as a background [`tokio::task`].
pub async fn agent_loop(
    event_tx: mpsc::Sender<Message>,
    client: Arc<dyn LlmClient>,
    registry: Arc<ToolRegistry>,
    mut cmd_rx: mpsc::Receiver<AgentCommand>,
    system_prompt: Option<String>,
) {
    info!("legacy agent loop started (model={})", client.model_name());

    let mut history: Vec<ChatMessage> = Vec::new();

    if let Some(sys) = system_prompt {
        history.push(ChatMessage::system(sys));
    }

    loop {
        match cmd_rx.recv().await {
            Some(AgentCommand::UserMessage(text)) => {
                if let Err(e) =
                    run_turn(&text, &event_tx, &client, &registry, &mut cmd_rx, &mut history)
                        .await
                {
                    error!("agent turn error: {e}");
                    send_event(&event_tx, AgentEvent::Error(e.to_string())).await;
                }
            }
            Some(AgentCommand::Cancel) => {
                debug!("agent loop received Cancel before any message; ignoring");
            }
            Some(AgentCommand::Approve(_)) => {
                debug!("stray Approve with no pending tool call; ignoring");
            }
            None => {
                info!("agent command channel closed; shutting down");
                break;
            }
        }
    }
}

async fn send_event(tx: &mpsc::Sender<Message>, event: AgentEvent) {
    if tx.send(Message::Agent(event)).await.is_err() {
        debug!("event receiver dropped");
    }
}

async fn run_turn(
    user_text: &str,
    event_tx: &mpsc::Sender<Message>,
    client: &Arc<dyn LlmClient>,
    registry: &Arc<ToolRegistry>,
    cmd_rx: &mut mpsc::Receiver<AgentCommand>,
    history: &mut Vec<ChatMessage>,
) -> Result<()> {
    history.push(ChatMessage::user(user_text));

    let mut state = AgentState::Idle;
    let mut tool_rounds = 0;

    loop {
        state = state.start_thinking()?;

        let request = ChatRequest {
            model: client.model_name().to_string(),
            messages: history.clone(),
            stream: true,
            max_tokens: None,
            temperature: None,
        };

        let (chunk_tx, mut chunk_rx) = mpsc::channel::<StreamChunk>(256);
        let client_clone = client.clone();
        let request_clone = request.clone();

        let stream_task = tokio::spawn(async move {
            client_clone.chat_stream(request_clone, chunk_tx).await
        });

        let mut accumulator = StreamAccumulator::new();
        let mut streamed_tool_calls: Vec<crate::legacy::ollama::types::ToolCall> = Vec::new();

        while let Some(chunk) = chunk_rx.recv().await {
            streamed_tool_calls.extend(chunk.tool_calls.clone());

            if !chunk.content.is_empty() {
                if let Some(emittable) = accumulator.push(&chunk.content) {
                    send_event(event_tx, AgentEvent::Token(emittable)).await;
                }
            }

            if chunk.done {
                if let Some(remaining) = accumulator.flush() {
                    send_event(event_tx, AgentEvent::Token(remaining)).await;
                }
                break;
            }
        }

        let response = match stream_task.await {
            Ok(Ok(r)) => r,
            Ok(Err(e)) => {
                send_event(event_tx, AgentEvent::Error(e.to_string())).await;
                return Err(e);
            }
            Err(join_err) => {
                let msg = format!("stream task panicked: {join_err}");
                send_event(event_tx, AgentEvent::Error(msg.clone())).await;
                anyhow::bail!(msg);
            }
        };

        if let Some(tc) = &response.message.tool_calls {
            for call in tc {
                if !streamed_tool_calls.iter().any(|s| s.function.name == call.function.name) {
                    streamed_tool_calls.push(call.clone());
                }
            }
        }

        history.push(response.message.clone());

        if streamed_tool_calls.is_empty() {
            let _ = state.complete()?;
            send_event(event_tx, AgentEvent::ResponseComplete).await;
            return Ok(());
        }

        tool_rounds += 1;
        if tool_rounds > MAX_TOOL_ROUNDS {
            warn!("exceeded max tool rounds ({MAX_TOOL_ROUNDS}); breaking loop");
            let _ = state.complete()?;
            send_event(event_tx, AgentEvent::ResponseComplete).await;
            return Ok(());
        }

        for tool_call in streamed_tool_calls {
            let tool_name = tool_call.function.name.clone();
            let args = tool_call.function.arguments.clone();

            let tool = match registry.get(&tool_name) {
                Some(t) => t,
                None => {
                    warn!("LLM requested unknown tool: {tool_name}");
                    let err_msg = format!("Tool `{tool_name}` not found.");
                    history.push(ChatMessage::tool_result(&tool_name, &err_msg));
                    send_event(event_tx, AgentEvent::ToolComplete {
                        tool_name: tool_name.clone(),
                        output: err_msg,
                    }).await;
                    continue;
                }
            };

            let output = if tool.requires_approval() {
                send_event(event_tx, AgentEvent::ToolCallRequested {
                    tool_name: tool_name.clone(),
                    args: args.clone(),
                }).await;

                state = state.request_approval(&tool_name, &args)?;
                let args_display = serde_json::to_string_pretty(&args)
                    .unwrap_or_else(|_| args.to_string());
                send_event(event_tx, AgentEvent::ApprovalRequired {
                    tool_name: tool_name.clone(),
                    args: args_display,
                }).await;

                let approved = wait_for_approval(&tool_name, cmd_rx).await;

                if approved {
                    debug!("tool approved: {tool_name}");
                    execute_tool_legacy(tool.as_ref(), &args).await
                } else {
                    debug!("tool denied: {tool_name}");
                    Ok(format!("User denied execution of `{tool_name}`."))
                }
            } else {
                send_event(event_tx, AgentEvent::ToolCallRequested {
                    tool_name: tool_name.clone(),
                    args: args.clone(),
                }).await;
                state = state.start_tool_call(&tool_name)?;
                execute_tool_legacy(tool.as_ref(), &args).await
            };

            match output {
                Ok(result) => {
                    history.push(ChatMessage::tool_result(&tool_name, &result));
                    send_event(event_tx, AgentEvent::ToolComplete {
                        tool_name: tool_name.clone(),
                        output: result,
                    }).await;
                }
                Err(e) => {
                    let err_str = format!("Tool `{tool_name}` failed: {e}");
                    history.push(ChatMessage::tool_result(&tool_name, &err_str));
                    send_event(event_tx, AgentEvent::ToolComplete {
                        tool_name: tool_name.clone(),
                        output: err_str,
                    }).await;
                }
            }

            state = state.complete()?;
        }
    }
}

async fn wait_for_approval(
    tool_name: &str,
    cmd_rx: &mut mpsc::Receiver<AgentCommand>,
) -> bool {
    debug!("waiting for approval of tool: {tool_name}");
    loop {
        match cmd_rx.recv().await {
            Some(AgentCommand::Approve(decision)) => return decision,
            Some(AgentCommand::Cancel) => {
                info!("agent turn cancelled while waiting for approval");
                return false;
            }
            Some(AgentCommand::UserMessage(_)) => {
                warn!("user sent message while approval pending; treating as denial");
                return false;
            }
            None => {
                debug!("command channel closed while waiting for approval");
                return false;
            }
        }
    }
}

async fn execute_tool_legacy(
    tool: &dyn crate::legacy::tools::Tool,
    args: &serde_json::Value,
) -> Result<String> {
    tool.execute(args.clone()).await
}
