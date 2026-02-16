// ABOUTME: Tests for the Orchestrator run loop, deduplication, and message routing.
// ABOUTME: Validates that inbound bus messages are routed to dispatch or session handlers.

use std::sync::Arc;

use chrono::Utc;
use gorp::bus::{BusMessage, MessageBus, MessageSource, ResponseContent, SessionTarget};
use gorp::orchestrator::Orchestrator;
use tokio::time::{timeout, Duration};

/// Helper to create a BusMessage for testing.
fn make_bus_message(id: &str, body: &str, target: SessionTarget) -> BusMessage {
    BusMessage {
        id: id.to_string(),
        source: MessageSource::Web {
            connection_id: "test-conn".to_string(),
        },
        session_target: target,
        sender: "test-user".to_string(),
        body: body.to_string(),
        timestamp: Utc::now(),
    }
}

#[tokio::test]
async fn test_orchestrator_routes_dispatch_messages() {
    let bus = Arc::new(MessageBus::new(64));
    let orchestrator = Orchestrator::new(Arc::clone(&bus));

    // Subscribe to responses BEFORE spawning orchestrator
    let mut resp_rx = bus.subscribe_responses();

    // Spawn orchestrator run loop in background
    let orch = orchestrator.clone();
    let handle = tokio::spawn(async move {
        orch.run().await;
    });

    // Give the orchestrator time to subscribe
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Publish a !help message targeting Dispatch
    let msg = make_bus_message("msg-001", "!help", SessionTarget::Dispatch);
    bus.publish_inbound(msg);

    // Wait for a response
    let resp = timeout(Duration::from_secs(2), resp_rx.recv())
        .await
        .expect("timed out waiting for response")
        .expect("failed to receive response");

    // Should be a SystemNotice from DISPATCH
    assert_eq!(resp.session_name, "DISPATCH");
    match &resp.content {
        ResponseContent::SystemNotice(text) => {
            assert!(text.contains("help"), "Help response should mention help");
        }
        other => panic!("Expected SystemNotice, got {:?}", other),
    }

    handle.abort();
}

#[tokio::test]
async fn test_orchestrator_deduplicates_messages() {
    let bus = Arc::new(MessageBus::new(64));
    let orchestrator = Orchestrator::new(Arc::clone(&bus));

    let mut resp_rx = bus.subscribe_responses();

    let orch = orchestrator.clone();
    let handle = tokio::spawn(async move {
        orch.run().await;
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    // Publish the SAME message ID twice
    let msg1 = make_bus_message("msg-dup", "!help", SessionTarget::Dispatch);
    let msg2 = make_bus_message("msg-dup", "!help", SessionTarget::Dispatch);
    bus.publish_inbound(msg1);
    // Small delay to ensure ordering
    tokio::time::sleep(Duration::from_millis(20)).await;
    bus.publish_inbound(msg2);

    // We should get exactly ONE response
    let first = timeout(Duration::from_secs(2), resp_rx.recv())
        .await
        .expect("timed out waiting for first response")
        .expect("failed to receive first response");

    assert_eq!(first.session_name, "DISPATCH");

    // Second recv should time out (no second response)
    let second = timeout(Duration::from_millis(500), resp_rx.recv()).await;
    assert!(
        second.is_err(),
        "Should not get a second response for duplicate message ID"
    );

    handle.abort();
}

#[tokio::test]
async fn test_orchestrator_routes_session_messages() {
    let bus = Arc::new(MessageBus::new(64));
    let orchestrator = Orchestrator::new(Arc::clone(&bus));

    let mut resp_rx = bus.subscribe_responses();

    let orch = orchestrator.clone();
    let handle = tokio::spawn(async move {
        orch.run().await;
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    // Publish a message targeting a named session
    let msg = make_bus_message(
        "msg-session-001",
        "summarize the paper",
        SessionTarget::Session {
            name: "research".to_string(),
        },
    );
    bus.publish_inbound(msg);

    let resp = timeout(Duration::from_secs(2), resp_rx.recv())
        .await
        .expect("timed out waiting for response")
        .expect("failed to receive response");

    // Should come back from the named session with a stub notice
    assert_eq!(resp.session_name, "research");
    match &resp.content {
        ResponseContent::SystemNotice(text) => {
            assert!(
                text.to_lowercase().contains("not yet wired"),
                "Stub response should indicate not yet wired, got: {}",
                text
            );
        }
        other => panic!("Expected SystemNotice stub, got {:?}", other),
    }

    handle.abort();
}

#[tokio::test]
async fn test_orchestrator_help_command() {
    let bus = Arc::new(MessageBus::new(64));
    let orchestrator = Orchestrator::new(Arc::clone(&bus));

    let mut resp_rx = bus.subscribe_responses();

    let orch = orchestrator.clone();
    let handle = tokio::spawn(async move {
        orch.run().await;
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    let msg = make_bus_message("msg-help", "!help", SessionTarget::Dispatch);
    bus.publish_inbound(msg);

    let resp = timeout(Duration::from_secs(2), resp_rx.recv())
        .await
        .expect("timed out waiting for response")
        .expect("failed to receive response");

    assert_eq!(resp.session_name, "DISPATCH");
    match &resp.content {
        ResponseContent::SystemNotice(text) => {
            // Help text should list available commands
            assert!(text.contains("!create"), "Help should list !create");
            assert!(text.contains("!delete"), "Help should list !delete");
            assert!(text.contains("!list"), "Help should list !list");
            assert!(text.contains("!status"), "Help should list !status");
            assert!(text.contains("!join"), "Help should list !join");
            assert!(text.contains("!leave"), "Help should list !leave");
            assert!(text.contains("!tell"), "Help should list !tell");
            assert!(text.contains("!read"), "Help should list !read");
            assert!(text.contains("!broadcast"), "Help should list !broadcast");
            assert!(text.contains("!help"), "Help should list !help");
        }
        other => panic!("Expected SystemNotice with help text, got {:?}", other),
    }

    handle.abort();
}

#[tokio::test]
async fn test_orchestrator_unknown_command() {
    let bus = Arc::new(MessageBus::new(64));
    let orchestrator = Orchestrator::new(Arc::clone(&bus));

    let mut resp_rx = bus.subscribe_responses();

    let orch = orchestrator.clone();
    let handle = tokio::spawn(async move {
        orch.run().await;
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    // Send a non-command message (no ! prefix)
    let msg = make_bus_message("msg-unknown", "hello there", SessionTarget::Dispatch);
    bus.publish_inbound(msg);

    let resp = timeout(Duration::from_secs(2), resp_rx.recv())
        .await
        .expect("timed out waiting for response")
        .expect("failed to receive response");

    assert_eq!(resp.session_name, "DISPATCH");
    match &resp.content {
        ResponseContent::SystemNotice(text) => {
            let lower = text.to_lowercase();
            assert!(
                lower.contains("unknown command"),
                "Should indicate unknown command, got: {}",
                text
            );
            assert!(
                lower.contains("!help"),
                "Should hint to use !help, got: {}",
                text
            );
        }
        other => panic!("Expected SystemNotice about unknown command, got {:?}", other),
    }

    handle.abort();
}

#[tokio::test]
async fn test_orchestrator_list_command() {
    let bus = Arc::new(MessageBus::new(64));
    let orchestrator = Orchestrator::new(Arc::clone(&bus));

    let mut resp_rx = bus.subscribe_responses();

    let orch = orchestrator.clone();
    let handle = tokio::spawn(async move {
        orch.run().await;
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    let msg = make_bus_message("msg-list", "!list", SessionTarget::Dispatch);
    bus.publish_inbound(msg);

    let resp = timeout(Duration::from_secs(2), resp_rx.recv())
        .await
        .expect("timed out waiting for response")
        .expect("failed to receive response");

    assert_eq!(resp.session_name, "DISPATCH");
    match &resp.content {
        ResponseContent::SystemNotice(text) => {
            // !list is a stub for now
            assert!(
                !text.is_empty(),
                "!list response should not be empty"
            );
        }
        other => panic!("Expected SystemNotice for !list, got {:?}", other),
    }

    handle.abort();
}

#[tokio::test]
async fn test_orchestrator_wired_commands_return_stub() {
    let bus = Arc::new(MessageBus::new(64));
    let orchestrator = Orchestrator::new(Arc::clone(&bus));

    let mut resp_rx = bus.subscribe_responses();

    let orch = orchestrator.clone();
    let handle = tokio::spawn(async move {
        orch.run().await;
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    // Commands that are recognized but not yet wired (Task 7 will wire them)
    let msg = make_bus_message("msg-create", "!create research", SessionTarget::Dispatch);
    bus.publish_inbound(msg);

    let resp = timeout(Duration::from_secs(2), resp_rx.recv())
        .await
        .expect("timed out waiting for response")
        .expect("failed to receive response");

    assert_eq!(resp.session_name, "DISPATCH");
    match &resp.content {
        ResponseContent::SystemNotice(text) => {
            let lower = text.to_lowercase();
            assert!(
                lower.contains("not yet wired"),
                "Recognized but unwired command should say 'not yet wired', got: {}",
                text
            );
        }
        other => panic!("Expected SystemNotice stub for !create, got {:?}", other),
    }

    handle.abort();
}
