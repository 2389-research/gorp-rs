// ABOUTME: Tests for core message bus types (BusMessage, BusResponse, MessageSource, etc.).
// ABOUTME: Validates construction, pattern matching, and variant coverage for bus primitives.

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
