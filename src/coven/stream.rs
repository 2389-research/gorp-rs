// ABOUTME: Per-agent gRPC stream handling for coven gateway communication.
// ABOUTME: Maps AgentEvent to MessageResponse and routes SendMessage to agent backends.

use std::collections::HashMap;

use gorp_agent::{AgentEvent, AgentHandle};
use tokio::sync::mpsc;

use super::proto;
use proto::agent_message::Payload;
use proto::message_response::Event;
use proto::{AgentMessage, MessageResponse};

/// Map an AgentEvent to one or more AgentMessage responses for the gateway
pub fn map_event_to_responses(request_id: &str, event: AgentEvent) -> Vec<AgentMessage> {
    let mut messages = Vec::new();

    match event {
        AgentEvent::Text(text) => {
            messages.push(response_msg(request_id, Event::Text(text)));
        }
        AgentEvent::ToolStart { id, name, input } => {
            messages.push(response_msg(
                request_id,
                Event::ToolUse(proto::ToolUse {
                    id,
                    name,
                    input_json: input.to_string(),
                }),
            ));
        }
        AgentEvent::ToolEnd {
            id,
            output,
            success,
            ..
        } => {
            messages.push(response_msg(
                request_id,
                Event::ToolResult(proto::ToolResult {
                    id,
                    output: output.to_string(),
                    is_error: !success,
                }),
            ));
        }
        AgentEvent::ToolProgress { id, .. } => {
            messages.push(response_msg(
                request_id,
                Event::ToolState(proto::ToolStateUpdate {
                    id,
                    state: proto::ToolState::Running as i32,
                    detail: None,
                }),
            ));
        }
        AgentEvent::Result { text, usage, .. } => {
            // Send usage before done if available
            if let Some(u) = usage {
                messages.push(response_msg(
                    request_id,
                    Event::Usage(proto::TokenUsage {
                        input_tokens: u.input_tokens.min(i32::MAX as u64) as i32,
                        output_tokens: u.output_tokens.min(i32::MAX as u64) as i32,
                        cache_read_tokens: u.cache_read_tokens.unwrap_or(0).min(i32::MAX as u64) as i32,
                        cache_write_tokens: u.cache_write_tokens.unwrap_or(0).min(i32::MAX as u64) as i32,
                        thinking_tokens: 0,
                    }),
                ));
            }
            messages.push(response_msg(
                request_id,
                Event::Done(proto::Done {
                    full_response: text,
                }),
            ));
        }
        AgentEvent::Error { message, .. } => {
            messages.push(response_msg(request_id, Event::Error(message)));
        }
        AgentEvent::SessionChanged { new_session_id } => {
            messages.push(response_msg(
                request_id,
                Event::SessionInit(proto::SessionInit {
                    session_id: new_session_id,
                }),
            ));
        }
        AgentEvent::SessionInvalid { reason } => {
            messages.push(response_msg(
                request_id,
                Event::SessionOrphaned(proto::SessionOrphaned { reason }),
            ));
        }
        AgentEvent::Custom { kind, .. } => {
            tracing::trace!(kind = %kind, "Unmapped custom agent event");
        }
    }

    messages
}

/// Construct an AgentMessage wrapping a MessageResponse
fn response_msg(request_id: &str, event: Event) -> AgentMessage {
    AgentMessage {
        payload: Some(Payload::Response(MessageResponse {
            request_id: request_id.to_string(),
            event: Some(event),
        })),
    }
}

/// Handle a SendMessage by routing to an agent backend and streaming responses
pub async fn handle_send_message(
    send_msg: &proto::SendMessage,
    agent_handle: &AgentHandle,
    sessions: &mut HashMap<String, String>,
    tx: &mpsc::Sender<AgentMessage>,
) -> anyhow::Result<()> {
    let request_id = &send_msg.request_id;
    let thread_id = &send_msg.thread_id;

    // Get or create a session for this conversation thread
    let session_id = match sessions.get(thread_id) {
        Some(sid) => sid.clone(),
        None => {
            let sid = agent_handle.new_session().await?;
            tracing::info!(
                thread_id = %thread_id,
                session_id = %sid,
                "Created new agent session for coven thread"
            );
            sessions.insert(thread_id.to_string(), sid.clone());

            // Notify gateway of session initialization
            let init = response_msg(
                request_id,
                Event::SessionInit(proto::SessionInit {
                    session_id: sid.clone(),
                }),
            );
            let _ = tx.send(init).await;
            sid
        }
    };

    // Send prompt and stream responses back
    let mut event_rx = agent_handle.prompt(&session_id, &send_msg.content).await?;

    while let Some(event) = event_rx.recv().await {
        let responses = map_event_to_responses(request_id, event);
        for msg in responses {
            if tx.send(msg).await.is_err() {
                tracing::warn!(
                    request_id = %request_id,
                    "Response send failed, gRPC stream closed"
                );
                return Ok(());
            }
        }
    }

    Ok(())
}

/// Handle a SendMessage for DISPATCH by routing through the dispatch system
pub async fn handle_dispatch_message(
    send_msg: &proto::SendMessage,
    agent_handle: &AgentHandle,
    sessions: &mut HashMap<String, String>,
    session_store: &crate::session::SessionStore,
    tx: &mpsc::Sender<AgentMessage>,
) -> anyhow::Result<()> {
    let request_id = &send_msg.request_id;
    let thread_id = &send_msg.thread_id;

    // Get or create a session for this DISPATCH thread
    let session_id = match sessions.get(thread_id) {
        Some(sid) => sid.clone(),
        None => {
            let sid = agent_handle.new_session().await?;
            tracing::info!(
                thread_id = %thread_id,
                session_id = %sid,
                "Created new DISPATCH session for coven thread"
            );
            sessions.insert(thread_id.to_string(), sid.clone());

            // Notify gateway of session initialization
            let init = response_msg(
                request_id,
                Event::SessionInit(proto::SessionInit {
                    session_id: sid.clone(),
                }),
            );
            let _ = tx.send(init).await;
            sid
        }
    };

    // Generate dynamic system prompt with current state
    let system_prompt = crate::dispatch_system_prompt::generate_dispatch_prompt(session_store);

    // Prepend system context to the user message
    let full_prompt = format!(
        "<system>\n{}\n</system>\n\n<user_message>\n{}\n</user_message>",
        system_prompt, send_msg.content
    );

    // Send prompt and stream responses back
    let mut event_rx = agent_handle.prompt(&session_id, &full_prompt).await?;

    while let Some(event) = event_rx.recv().await {
        let responses = map_event_to_responses(request_id, event);
        for msg in responses {
            if tx.send(msg).await.is_err() {
                tracing::warn!(
                    request_id = %request_id,
                    "DISPATCH response send failed, gRPC stream closed"
                );
                return Ok(());
            }
        }
    }

    Ok(())
}

/// Handle a CancelRequest by cancelling active sessions
pub async fn handle_cancel_request(
    cancel: &proto::CancelRequest,
    agent_handle: &AgentHandle,
    sessions: &HashMap<String, String>,
    tx: &mpsc::Sender<AgentMessage>,
) -> anyhow::Result<()> {
    let reason = cancel.reason.clone().unwrap_or_default();

    // Cancel all active sessions for this agent
    for (_thread_id, session_id) in sessions.iter() {
        if let Err(e) = agent_handle.cancel(session_id).await {
            tracing::warn!(
                session_id = %session_id,
                error = %e,
                "Failed to cancel agent session"
            );
        }
    }

    // Acknowledge cancellation
    let ack = response_msg(
        &cancel.request_id,
        Event::Cancelled(proto::Cancelled { reason }),
    );
    let _ = tx.send(ack).await;

    Ok(())
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use gorp_agent::event::{ErrorCode, Usage};
    use serde_json::json;

    fn extract_event(msg: &AgentMessage) -> &Event {
        match &msg.payload {
            Some(Payload::Response(resp)) => resp.event.as_ref().unwrap(),
            _ => panic!("expected Response payload"),
        }
    }

    fn extract_request_id(msg: &AgentMessage) -> &str {
        match &msg.payload {
            Some(Payload::Response(resp)) => &resp.request_id,
            _ => panic!("expected Response payload"),
        }
    }

    #[test]
    fn test_map_text_event() {
        let events = map_event_to_responses("req-1", AgentEvent::Text("hello".to_string()));
        assert_eq!(events.len(), 1);
        assert_eq!(extract_request_id(&events[0]), "req-1");
        match extract_event(&events[0]) {
            Event::Text(t) => assert_eq!(t, "hello"),
            other => panic!("expected Text, got {:?}", other),
        }
    }

    #[test]
    fn test_map_tool_start() {
        let events = map_event_to_responses(
            "req-2",
            AgentEvent::ToolStart {
                id: "tool-1".to_string(),
                name: "Read".to_string(),
                input: json!({"file": "test.rs"}),
            },
        );
        assert_eq!(events.len(), 1);
        match extract_event(&events[0]) {
            Event::ToolUse(tu) => {
                assert_eq!(tu.id, "tool-1");
                assert_eq!(tu.name, "Read");
                assert!(tu.input_json.contains("test.rs"));
            }
            other => panic!("expected ToolUse, got {:?}", other),
        }
    }

    #[test]
    fn test_map_tool_end_success() {
        let events = map_event_to_responses(
            "req-3",
            AgentEvent::ToolEnd {
                id: "tool-1".to_string(),
                name: "Read".to_string(),
                output: json!("file contents"),
                success: true,
                duration_ms: 42,
            },
        );
        assert_eq!(events.len(), 1);
        match extract_event(&events[0]) {
            Event::ToolResult(tr) => {
                assert_eq!(tr.id, "tool-1");
                assert!(!tr.is_error);
            }
            other => panic!("expected ToolResult, got {:?}", other),
        }
    }

    #[test]
    fn test_map_tool_end_failure() {
        let events = map_event_to_responses(
            "req-4",
            AgentEvent::ToolEnd {
                id: "tool-2".to_string(),
                name: "Bash".to_string(),
                output: json!("error output"),
                success: false,
                duration_ms: 100,
            },
        );
        match extract_event(&events[0]) {
            Event::ToolResult(tr) => assert!(tr.is_error),
            other => panic!("expected ToolResult, got {:?}", other),
        }
    }

    #[test]
    fn test_map_tool_progress() {
        let events = map_event_to_responses(
            "req-11",
            AgentEvent::ToolProgress {
                id: "tool-5".to_string(),
                update: json!({"progress": 50}),
            },
        );
        assert_eq!(events.len(), 1);
        match extract_event(&events[0]) {
            Event::ToolState(ts) => {
                assert_eq!(ts.id, "tool-5");
                assert_eq!(ts.state, proto::ToolState::Running as i32);
            }
            other => panic!("expected ToolState, got {:?}", other),
        }
    }

    #[test]
    fn test_map_result_with_usage() {
        let events = map_event_to_responses(
            "req-5",
            AgentEvent::Result {
                text: "final answer".to_string(),
                usage: Some(Usage {
                    input_tokens: 100,
                    output_tokens: 50,
                    cache_read_tokens: Some(20),
                    cache_write_tokens: Some(10),
                    cost_usd: Some(0.005),
                    extra: None,
                }),
                metadata: json!({}),
            },
        );
        // Should produce two messages: usage then done
        assert_eq!(events.len(), 2);
        match extract_event(&events[0]) {
            Event::Usage(u) => {
                assert_eq!(u.input_tokens, 100);
                assert_eq!(u.output_tokens, 50);
                assert_eq!(u.cache_read_tokens, 20);
                assert_eq!(u.cache_write_tokens, 10);
            }
            other => panic!("expected Usage, got {:?}", other),
        }
        match extract_event(&events[1]) {
            Event::Done(d) => assert_eq!(d.full_response, "final answer"),
            other => panic!("expected Done, got {:?}", other),
        }
    }

    #[test]
    fn test_map_result_without_usage() {
        let events = map_event_to_responses(
            "req-6",
            AgentEvent::Result {
                text: "answer".to_string(),
                usage: None,
                metadata: json!({}),
            },
        );
        assert_eq!(events.len(), 1);
        match extract_event(&events[0]) {
            Event::Done(d) => assert_eq!(d.full_response, "answer"),
            other => panic!("expected Done, got {:?}", other),
        }
    }

    #[test]
    fn test_map_error() {
        let events = map_event_to_responses(
            "req-7",
            AgentEvent::Error {
                code: ErrorCode::BackendError,
                message: "something went wrong".to_string(),
                recoverable: false,
            },
        );
        assert_eq!(events.len(), 1);
        match extract_event(&events[0]) {
            Event::Error(e) => assert_eq!(e, "something went wrong"),
            other => panic!("expected Error, got {:?}", other),
        }
    }

    #[test]
    fn test_map_session_changed() {
        let events = map_event_to_responses(
            "req-8",
            AgentEvent::SessionChanged {
                new_session_id: "new-sid-123".to_string(),
            },
        );
        assert_eq!(events.len(), 1);
        match extract_event(&events[0]) {
            Event::SessionInit(si) => assert_eq!(si.session_id, "new-sid-123"),
            other => panic!("expected SessionInit, got {:?}", other),
        }
    }

    #[test]
    fn test_map_session_invalid() {
        let events = map_event_to_responses(
            "req-9",
            AgentEvent::SessionInvalid {
                reason: "expired".to_string(),
            },
        );
        assert_eq!(events.len(), 1);
        match extract_event(&events[0]) {
            Event::SessionOrphaned(so) => assert_eq!(so.reason, "expired"),
            other => panic!("expected SessionOrphaned, got {:?}", other),
        }
    }

    #[test]
    fn test_map_custom_produces_nothing() {
        let events = map_event_to_responses(
            "req-10",
            AgentEvent::Custom {
                kind: "acp.internal".to_string(),
                payload: json!({}),
            },
        );
        assert!(events.is_empty());
    }

    #[test]
    fn test_response_msg_structure() {
        let msg = response_msg("test-req", Event::Text("hi".to_string()));
        match &msg.payload {
            Some(Payload::Response(resp)) => {
                assert_eq!(resp.request_id, "test-req");
                assert!(resp.event.is_some());
            }
            _ => panic!("expected Response payload"),
        }
    }
}
