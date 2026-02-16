// ABOUTME: Tests for the WebAdapter gateway that bridges admin WebSocket connections and the message bus.
// ABOUTME: Validates platform_id, outbound routing (chunk/complete/error/system notice), and direct send.

use gorp::admin::websocket::{ServerMessage, WsHub};
use gorp::bus::{BusResponse, MessageBus, ResponseContent};
use gorp::gateway::web::WebAdapter;
use gorp::gateway::GatewayAdapter;
use std::sync::Arc;

#[tokio::test]
async fn test_web_adapter_platform_id() {
    let hub = WsHub::new();
    let adapter = WebAdapter::new(hub);
    assert_eq!(adapter.platform_id(), "web");
}

#[tokio::test]
async fn test_web_adapter_outbound_routes_chunk_to_ws_hub() {
    let hub = WsHub::new();
    let mut rx = hub.subscribe();
    let adapter = WebAdapter::new(hub);
    let bus = Arc::new(MessageBus::new(64));

    adapter.start(bus.clone()).await.unwrap();

    // Give the spawned task a moment to subscribe
    tokio::task::yield_now().await;

    bus.publish_response(BusResponse {
        session_name: "research".to_string(),
        content: ResponseContent::Chunk("hello world".to_string()),
        timestamp: chrono::Utc::now(),
    });

    let msg = tokio::time::timeout(std::time::Duration::from_secs(2), rx.recv())
        .await
        .expect("timed out waiting for hub message")
        .expect("recv error");

    match msg {
        ServerMessage::ChatChunk { data } => {
            assert_eq!(data.workspace, "research");
            assert_eq!(data.text, "hello world");
        }
        other => panic!("Expected ChatChunk, got {:?}", other),
    }
}

#[tokio::test]
async fn test_web_adapter_outbound_complete() {
    let hub = WsHub::new();
    let mut rx = hub.subscribe();
    let adapter = WebAdapter::new(hub);
    let bus = Arc::new(MessageBus::new(64));

    adapter.start(bus.clone()).await.unwrap();
    tokio::task::yield_now().await;

    bus.publish_response(BusResponse {
        session_name: "devops".to_string(),
        content: ResponseContent::Complete("final answer".to_string()),
        timestamp: chrono::Utc::now(),
    });

    let msg = tokio::time::timeout(std::time::Duration::from_secs(2), rx.recv())
        .await
        .expect("timed out waiting for hub message")
        .expect("recv error");

    match msg {
        ServerMessage::ChatComplete { data } => {
            assert_eq!(data.workspace, "devops");
        }
        other => panic!("Expected ChatComplete, got {:?}", other),
    }
}

#[tokio::test]
async fn test_web_adapter_outbound_error() {
    let hub = WsHub::new();
    let mut rx = hub.subscribe();
    let adapter = WebAdapter::new(hub);
    let bus = Arc::new(MessageBus::new(64));

    adapter.start(bus.clone()).await.unwrap();
    tokio::task::yield_now().await;

    bus.publish_response(BusResponse {
        session_name: "research".to_string(),
        content: ResponseContent::Error("backend timeout".to_string()),
        timestamp: chrono::Utc::now(),
    });

    let msg = tokio::time::timeout(std::time::Duration::from_secs(2), rx.recv())
        .await
        .expect("timed out waiting for hub message")
        .expect("recv error");

    match msg {
        ServerMessage::ChatError { data } => {
            assert_eq!(data.workspace, "research");
            assert_eq!(data.error, "backend timeout");
        }
        other => panic!("Expected ChatError, got {:?}", other),
    }
}

#[tokio::test]
async fn test_web_adapter_outbound_system_notice_sends_chunk_then_complete() {
    let hub = WsHub::new();
    let mut rx = hub.subscribe();
    let adapter = WebAdapter::new(hub);
    let bus = Arc::new(MessageBus::new(64));

    adapter.start(bus.clone()).await.unwrap();
    tokio::task::yield_now().await;

    bus.publish_response(BusResponse {
        session_name: "dispatch".to_string(),
        content: ResponseContent::SystemNotice("Session created".to_string()),
        timestamp: chrono::Utc::now(),
    });

    // SystemNotice should produce a ChatChunk followed by a ChatComplete
    let msg1 = tokio::time::timeout(std::time::Duration::from_secs(2), rx.recv())
        .await
        .expect("timed out waiting for chunk")
        .expect("recv error");

    match msg1 {
        ServerMessage::ChatChunk { data } => {
            assert_eq!(data.workspace, "dispatch");
            assert_eq!(data.text, "Session created");
        }
        other => panic!("Expected ChatChunk for SystemNotice, got {:?}", other),
    }

    let msg2 = tokio::time::timeout(std::time::Duration::from_secs(2), rx.recv())
        .await
        .expect("timed out waiting for complete")
        .expect("recv error");

    match msg2 {
        ServerMessage::ChatComplete { data } => {
            assert_eq!(data.workspace, "dispatch");
        }
        other => panic!("Expected ChatComplete after SystemNotice, got {:?}", other),
    }
}

#[tokio::test]
async fn test_web_adapter_send_chunk() {
    let hub = WsHub::new();
    let mut rx = hub.subscribe();
    let adapter = WebAdapter::new(hub);

    adapter
        .send("my-workspace", ResponseContent::Chunk("direct chunk".to_string()))
        .await
        .unwrap();

    let msg = rx.try_recv().unwrap();
    match msg {
        ServerMessage::ChatChunk { data } => {
            assert_eq!(data.workspace, "my-workspace");
            assert_eq!(data.text, "direct chunk");
        }
        other => panic!("Expected ChatChunk from send(), got {:?}", other),
    }
}

#[tokio::test]
async fn test_web_adapter_send_complete() {
    let hub = WsHub::new();
    let mut rx = hub.subscribe();
    let adapter = WebAdapter::new(hub);

    adapter
        .send("my-workspace", ResponseContent::Complete("done".to_string()))
        .await
        .unwrap();

    let msg = rx.try_recv().unwrap();
    match msg {
        ServerMessage::ChatComplete { data } => {
            assert_eq!(data.workspace, "my-workspace");
        }
        other => panic!("Expected ChatComplete from send(), got {:?}", other),
    }
}

#[tokio::test]
async fn test_web_adapter_send_error() {
    let hub = WsHub::new();
    let mut rx = hub.subscribe();
    let adapter = WebAdapter::new(hub);

    adapter
        .send("my-workspace", ResponseContent::Error("oops".to_string()))
        .await
        .unwrap();

    let msg = rx.try_recv().unwrap();
    match msg {
        ServerMessage::ChatError { data } => {
            assert_eq!(data.workspace, "my-workspace");
            assert_eq!(data.error, "oops");
        }
        other => panic!("Expected ChatError from send(), got {:?}", other),
    }
}

#[tokio::test]
async fn test_web_adapter_send_system_notice() {
    let hub = WsHub::new();
    let mut rx = hub.subscribe();
    let adapter = WebAdapter::new(hub);

    adapter
        .send(
            "my-workspace",
            ResponseContent::SystemNotice("notice text".to_string()),
        )
        .await
        .unwrap();

    // send() for SystemNotice should produce a ChatChunk then ChatComplete
    let msg1 = rx.try_recv().unwrap();
    match msg1 {
        ServerMessage::ChatChunk { data } => {
            assert_eq!(data.workspace, "my-workspace");
            assert_eq!(data.text, "notice text");
        }
        other => panic!("Expected ChatChunk from send() SystemNotice, got {:?}", other),
    }

    let msg2 = rx.try_recv().unwrap();
    match msg2 {
        ServerMessage::ChatComplete { data } => {
            assert_eq!(data.workspace, "my-workspace");
        }
        other => panic!(
            "Expected ChatComplete from send() SystemNotice, got {:?}",
            other
        ),
    }
}

#[tokio::test]
async fn test_web_adapter_stop_succeeds() {
    let hub = WsHub::new();
    let adapter = WebAdapter::new(hub);
    assert!(adapter.stop().await.is_ok());
}
