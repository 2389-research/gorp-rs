// ABOUTME: Tests for core message bus types and MessageBus pub/sub infrastructure.
// ABOUTME: Validates types, pattern matching, broadcast channels, and channel-to-session bindings.

use chrono::Utc;
use gorp::bus::*;

#[test]
fn test_bus_message_dispatch_target() {
    let msg = BusMessage {
        id: "evt-1".to_string(),
        source: MessageSource::Web {
            connection_id: "ws-42".to_string(),
        },
        session_target: SessionTarget::Dispatch,
        sender: "harper".to_string(),
        body: "!create research".to_string(),
        timestamp: Utc::now(),
    };
    assert!(matches!(msg.session_target, SessionTarget::Dispatch));
    assert_eq!(msg.sender, "harper");
}

#[test]
fn test_bus_message_session_target() {
    let msg = BusMessage {
        id: "evt-2".to_string(),
        source: MessageSource::Platform {
            platform_id: "matrix".to_string(),
            channel_id: "!room123:matrix.org".to_string(),
        },
        session_target: SessionTarget::Session {
            name: "research".to_string(),
        },
        sender: "harper".to_string(),
        body: "summarize the paper".to_string(),
        timestamp: Utc::now(),
    };
    assert!(
        matches!(msg.session_target, SessionTarget::Session { ref name } if name == "research")
    );
}

#[test]
fn test_bus_response_chunk() {
    let resp = BusResponse {
        session_name: "research".to_string(),
        content: ResponseContent::Chunk("partial output...".to_string()),
        timestamp: Utc::now(),
    };
    assert!(matches!(resp.content, ResponseContent::Chunk(_)));
}

#[test]
fn test_bus_response_complete() {
    let resp = BusResponse {
        session_name: "research".to_string(),
        content: ResponseContent::Complete("full response".to_string()),
        timestamp: Utc::now(),
    };
    assert!(matches!(resp.content, ResponseContent::Complete(_)));
}

#[test]
fn test_bus_response_system_notice() {
    let resp = BusResponse {
        session_name: "".to_string(),
        content: ResponseContent::SystemNotice("Session 'research' created".to_string()),
        timestamp: Utc::now(),
    };
    assert!(matches!(resp.content, ResponseContent::SystemNotice(_)));
}

#[test]
fn test_message_source_api() {
    let source = MessageSource::Api {
        token_hint: "sk-***abc".to_string(),
    };
    assert!(matches!(source, MessageSource::Api { .. }));
}

// --- MessageBus tests ---

#[tokio::test]
async fn test_message_bus_publish_and_receive() {
    let bus = MessageBus::new(64);
    let mut rx = bus.subscribe_inbound();
    let msg = BusMessage {
        id: "evt-1".to_string(),
        source: MessageSource::Web {
            connection_id: "ws-1".to_string(),
        },
        session_target: SessionTarget::Dispatch,
        sender: "harper".to_string(),
        body: "hello".to_string(),
        timestamp: Utc::now(),
    };
    bus.publish_inbound(msg);
    let received = rx.recv().await.unwrap();
    assert_eq!(received.id, "evt-1");
    assert_eq!(received.body, "hello");
}

#[tokio::test]
async fn test_message_bus_response_broadcast() {
    let bus = MessageBus::new(64);
    let mut rx1 = bus.subscribe_responses();
    let mut rx2 = bus.subscribe_responses();
    let resp = BusResponse {
        session_name: "research".to_string(),
        content: ResponseContent::Complete("done".to_string()),
        timestamp: Utc::now(),
    };
    bus.publish_response(resp);
    let r1 = rx1.recv().await.unwrap();
    let r2 = rx2.recv().await.unwrap();
    assert_eq!(r1.session_name, "research");
    assert_eq!(r2.session_name, "research");
}

#[tokio::test]
async fn test_message_bus_channel_binding() {
    let bus = MessageBus::new(64);
    let target = bus.resolve_target_async("matrix", "!room1:m.org").await;
    assert_eq!(target, SessionTarget::Dispatch);

    bus.bind_channel_async("matrix", "!room1:m.org", "research").await;
    let target = bus.resolve_target_async("matrix", "!room1:m.org").await;
    assert_eq!(
        target,
        SessionTarget::Session {
            name: "research".to_string()
        }
    );

    bus.unbind_channel_async("matrix", "!room1:m.org").await;
    let target = bus.resolve_target_async("matrix", "!room1:m.org").await;
    assert_eq!(target, SessionTarget::Dispatch);
}

#[tokio::test]
async fn test_message_bus_multiple_bindings_same_session() {
    let bus = MessageBus::new(64);
    bus.bind_channel_async("matrix", "!room1:m.org", "research").await;
    bus.bind_channel_async("slack", "C12345", "research").await;
    let t1 = bus.resolve_target_async("matrix", "!room1:m.org").await;
    let t2 = bus.resolve_target_async("slack", "C12345").await;
    assert_eq!(
        t1,
        SessionTarget::Session {
            name: "research".to_string()
        }
    );
    assert_eq!(
        t2,
        SessionTarget::Session {
            name: "research".to_string()
        }
    );
}

#[tokio::test]
async fn test_message_bus_list_bindings_for_session() {
    let bus = MessageBus::new(64);
    bus.bind_channel_async("matrix", "!room1:m.org", "research").await;
    bus.bind_channel_async("slack", "C12345", "research").await;
    bus.bind_channel_async("matrix", "!room2:m.org", "ops").await;
    let bindings = bus.bindings_for_session_async("research").await;
    assert_eq!(bindings.len(), 2);
    assert!(bindings.contains(&("matrix".to_string(), "!room1:m.org".to_string())));
    assert!(bindings.contains(&("slack".to_string(), "C12345".to_string())));
}

#[tokio::test]
async fn test_message_bus_load_bindings() {
    let bus = MessageBus::new(64);
    bus.load_bindings(vec![
        ("matrix".to_string(), "!room1:m.org".to_string(), "research".to_string()),
        ("slack".to_string(), "C12345".to_string(), "ops".to_string()),
    ])
    .await;
    let t1 = bus.resolve_target_async("matrix", "!room1:m.org").await;
    let t2 = bus.resolve_target_async("slack", "C12345").await;
    assert_eq!(
        t1,
        SessionTarget::Session {
            name: "research".to_string()
        }
    );
    assert_eq!(
        t2,
        SessionTarget::Session {
            name: "ops".to_string()
        }
    );
}
