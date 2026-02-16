// ABOUTME: Integration tests for Orchestrator wired to real SessionStore.
// ABOUTME: Tests DISPATCH commands (!create, !delete, !list, !status, !join, !leave, !tell) with real persistence.

use std::sync::Arc;

use chrono::Utc;
use gorp::bus::{BusMessage, MessageBus, MessageSource, ResponseContent, SessionTarget};
use gorp::orchestrator::Orchestrator;
use gorp_core::session::SessionStore;
use tempfile::TempDir;
use tokio::time::{timeout, Duration};

/// Helper: create a test orchestrator wired to a real in-memory SessionStore.
/// Returns both the Orchestrator and the TempDir handle (must be kept alive).
fn create_test_orchestrator(bus: Arc<MessageBus>) -> (Orchestrator, SessionStore, TempDir) {
    let tmp = TempDir::new().unwrap();
    let session_store = SessionStore::new(tmp.path()).unwrap();
    let orchestrator = Orchestrator::new(bus, session_store.clone(), None);
    (orchestrator, session_store, tmp)
}

/// Helper: create a BusMessage targeting DISPATCH from a Web source.
fn dispatch_msg(id: &str, body: &str) -> BusMessage {
    BusMessage {
        id: id.to_string(),
        source: MessageSource::Web {
            connection_id: "test-conn-1".to_string(),
        },
        session_target: SessionTarget::Dispatch,
        sender: "test-user".to_string(),
        body: body.to_string(),
        timestamp: Utc::now(),
    }
}

/// Helper: spawn orchestrator, wait for it to be ready, return the join handle.
async fn spawn_orchestrator(orchestrator: &Orchestrator) -> tokio::task::JoinHandle<()> {
    let orch = orchestrator.clone();
    let handle = tokio::spawn(async move {
        orch.run().await;
    });
    // Give the orchestrator time to subscribe to the bus
    tokio::time::sleep(Duration::from_millis(50)).await;
    handle
}

/// Helper: send a dispatch command and collect the response.
async fn send_and_recv(
    bus: &Arc<MessageBus>,
    resp_rx: &mut tokio::sync::broadcast::Receiver<gorp::bus::BusResponse>,
    msg: BusMessage,
) -> gorp::bus::BusResponse {
    bus.publish_inbound(msg);
    timeout(Duration::from_secs(2), resp_rx.recv())
        .await
        .expect("timed out waiting for response")
        .expect("failed to receive response")
}

/// Extract the text from a SystemNotice response, panicking if it's another variant.
fn notice_text(resp: &gorp::bus::BusResponse) -> &str {
    match &resp.content {
        ResponseContent::SystemNotice(text) => text.as_str(),
        other => panic!("Expected SystemNotice, got {:?}", other),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_dispatch_create_session() {
    let bus = Arc::new(MessageBus::new(64));
    let (orchestrator, session_store, _tmp) = create_test_orchestrator(Arc::clone(&bus));
    let mut resp_rx = bus.subscribe_responses();
    let handle = spawn_orchestrator(&orchestrator).await;

    let resp = send_and_recv(&bus, &mut resp_rx, dispatch_msg("c1", "!create research")).await;
    let text = notice_text(&resp);
    assert!(
        text.contains("research") && text.to_lowercase().contains("created"),
        "Expected creation confirmation, got: {}",
        text
    );

    // Verify session exists in store
    let channel = session_store.get_by_name("research").unwrap();
    assert!(channel.is_some(), "Session 'research' should exist in store");

    handle.abort();
}

#[tokio::test]
async fn test_dispatch_list_empty() {
    let bus = Arc::new(MessageBus::new(64));
    let (orchestrator, _store, _tmp) = create_test_orchestrator(Arc::clone(&bus));
    let mut resp_rx = bus.subscribe_responses();
    let handle = spawn_orchestrator(&orchestrator).await;

    let resp = send_and_recv(&bus, &mut resp_rx, dispatch_msg("l1", "!list")).await;
    let text = notice_text(&resp);
    assert!(
        text.to_lowercase().contains("no active sessions"),
        "Empty list should say no active sessions, got: {}",
        text
    );

    handle.abort();
}

#[tokio::test]
async fn test_dispatch_list_with_sessions() {
    let bus = Arc::new(MessageBus::new(64));
    let (orchestrator, session_store, _tmp) = create_test_orchestrator(Arc::clone(&bus));
    let mut resp_rx = bus.subscribe_responses();
    let handle = spawn_orchestrator(&orchestrator).await;

    // Create sessions directly in the store
    session_store
        .create_channel("alpha", "bus:alpha")
        .unwrap();
    session_store
        .create_channel("beta", "bus:beta")
        .unwrap();

    let resp = send_and_recv(&bus, &mut resp_rx, dispatch_msg("l2", "!list")).await;
    let text = notice_text(&resp);
    assert!(
        text.contains("alpha"),
        "List should contain 'alpha', got: {}",
        text
    );
    assert!(
        text.contains("beta"),
        "List should contain 'beta', got: {}",
        text
    );
    assert!(
        text.contains("2 session(s)"),
        "List should show count, got: {}",
        text
    );

    handle.abort();
}

#[tokio::test]
async fn test_dispatch_delete_session() {
    let bus = Arc::new(MessageBus::new(64));
    let (orchestrator, session_store, _tmp) = create_test_orchestrator(Arc::clone(&bus));
    let mut resp_rx = bus.subscribe_responses();
    let handle = spawn_orchestrator(&orchestrator).await;

    // Create a session first
    session_store
        .create_channel("doomed", "bus:doomed")
        .unwrap();
    assert!(session_store.get_by_name("doomed").unwrap().is_some());

    let resp = send_and_recv(&bus, &mut resp_rx, dispatch_msg("d1", "!delete doomed")).await;
    let text = notice_text(&resp);
    assert!(
        text.to_lowercase().contains("deleted"),
        "Should confirm deletion, got: {}",
        text
    );

    // Verify it's gone
    assert!(
        session_store.get_by_name("doomed").unwrap().is_none(),
        "Session 'doomed' should be deleted from store"
    );

    handle.abort();
}

#[tokio::test]
async fn test_dispatch_status() {
    let bus = Arc::new(MessageBus::new(64));
    let (orchestrator, session_store, _tmp) = create_test_orchestrator(Arc::clone(&bus));
    let mut resp_rx = bus.subscribe_responses();
    let handle = spawn_orchestrator(&orchestrator).await;

    session_store
        .create_channel("research", "bus:research")
        .unwrap();

    let resp = send_and_recv(
        &bus,
        &mut resp_rx,
        dispatch_msg("s1", "!status research"),
    )
    .await;
    let text = notice_text(&resp);
    assert!(
        text.contains("research"),
        "Status should show session name, got: {}",
        text
    );
    assert!(
        text.contains("Session ID"),
        "Status should show session ID label, got: {}",
        text
    );

    handle.abort();
}

#[tokio::test]
async fn test_dispatch_status_not_found() {
    let bus = Arc::new(MessageBus::new(64));
    let (orchestrator, _store, _tmp) = create_test_orchestrator(Arc::clone(&bus));
    let mut resp_rx = bus.subscribe_responses();
    let handle = spawn_orchestrator(&orchestrator).await;

    let resp = send_and_recv(
        &bus,
        &mut resp_rx,
        dispatch_msg("s2", "!status nonexistent"),
    )
    .await;
    let text = notice_text(&resp);
    assert!(
        text.to_lowercase().contains("not found"),
        "Should say not found, got: {}",
        text
    );

    handle.abort();
}

#[tokio::test]
async fn test_dispatch_join_and_leave() {
    let bus = Arc::new(MessageBus::new(64));
    let (orchestrator, session_store, _tmp) = create_test_orchestrator(Arc::clone(&bus));
    let mut resp_rx = bus.subscribe_responses();
    let handle = spawn_orchestrator(&orchestrator).await;

    // Create a session first
    session_store
        .create_channel("research", "bus:research")
        .unwrap();

    // Join
    let resp = send_and_recv(
        &bus,
        &mut resp_rx,
        dispatch_msg("j1", "!join research"),
    )
    .await;
    let text = notice_text(&resp);
    assert!(
        text.to_lowercase().contains("bound"),
        "Should confirm binding, got: {}",
        text
    );

    // Verify binding in store
    let binding = session_store
        .resolve_binding("web", "test-conn-1")
        .unwrap();
    assert_eq!(
        binding,
        Some("research".to_string()),
        "Store should have the binding"
    );

    // Verify binding in bus
    let target = bus.resolve_target_async("web", "test-conn-1").await;
    assert_eq!(
        target,
        SessionTarget::Session {
            name: "research".to_string()
        },
        "Bus should have the binding"
    );

    // Leave
    let resp = send_and_recv(&bus, &mut resp_rx, dispatch_msg("j2", "!leave")).await;
    let text = notice_text(&resp);
    assert!(
        text.to_lowercase().contains("unbound"),
        "Should confirm unbinding, got: {}",
        text
    );

    // Verify binding removed from store
    let binding = session_store
        .resolve_binding("web", "test-conn-1")
        .unwrap();
    assert_eq!(binding, None, "Store binding should be removed");

    // Verify binding removed from bus
    let target = bus.resolve_target_async("web", "test-conn-1").await;
    assert_eq!(
        target,
        SessionTarget::Dispatch,
        "Bus binding should be removed"
    );

    handle.abort();
}

#[tokio::test]
async fn test_dispatch_join_nonexistent_session() {
    let bus = Arc::new(MessageBus::new(64));
    let (orchestrator, _store, _tmp) = create_test_orchestrator(Arc::clone(&bus));
    let mut resp_rx = bus.subscribe_responses();
    let handle = spawn_orchestrator(&orchestrator).await;

    let resp = send_and_recv(
        &bus,
        &mut resp_rx,
        dispatch_msg("j3", "!join ghost"),
    )
    .await;
    let text = notice_text(&resp);
    assert!(
        text.to_lowercase().contains("not found"),
        "Should say session not found, got: {}",
        text
    );

    handle.abort();
}

#[tokio::test]
async fn test_dispatch_tell() {
    let bus = Arc::new(MessageBus::new(64));
    let (orchestrator, session_store, _tmp) = create_test_orchestrator(Arc::clone(&bus));
    let mut resp_rx = bus.subscribe_responses();

    // Subscribe to inbound messages to catch the relayed message
    let mut inbound_rx = bus.subscribe_inbound();

    let handle = spawn_orchestrator(&orchestrator).await;

    // Create a session first
    session_store
        .create_channel("research", "bus:research")
        .unwrap();

    let resp = send_and_recv(
        &bus,
        &mut resp_rx,
        dispatch_msg("t1", "!tell research hello from dispatch"),
    )
    .await;
    let text = notice_text(&resp);
    assert!(
        text.to_lowercase().contains("sent"),
        "Should confirm message sent, got: {}",
        text
    );

    // The tell command should have published a new inbound message targeting the session.
    // We need to drain inbound messages to find it. The first message is our !tell command,
    // the second should be the relayed message.
    let mut found_relay = false;
    for _ in 0..10 {
        match timeout(Duration::from_millis(500), inbound_rx.recv()).await {
            Ok(Ok(msg)) => {
                if msg.session_target
                    == (SessionTarget::Session {
                        name: "research".to_string(),
                    })
                    && msg.body == "hello from dispatch"
                {
                    found_relay = true;
                    break;
                }
            }
            _ => break,
        }
    }
    assert!(
        found_relay,
        "Should have found the relayed message on the inbound bus"
    );

    handle.abort();
}

#[tokio::test]
async fn test_dispatch_tell_nonexistent_session() {
    let bus = Arc::new(MessageBus::new(64));
    let (orchestrator, _store, _tmp) = create_test_orchestrator(Arc::clone(&bus));
    let mut resp_rx = bus.subscribe_responses();
    let handle = spawn_orchestrator(&orchestrator).await;

    let resp = send_and_recv(
        &bus,
        &mut resp_rx,
        dispatch_msg("t2", "!tell ghost some message"),
    )
    .await;
    let text = notice_text(&resp);
    assert!(
        text.to_lowercase().contains("not found"),
        "Should say session not found, got: {}",
        text
    );

    handle.abort();
}

#[tokio::test]
async fn test_dispatch_read() {
    let bus = Arc::new(MessageBus::new(64));
    let (orchestrator, session_store, _tmp) = create_test_orchestrator(Arc::clone(&bus));
    let mut resp_rx = bus.subscribe_responses();
    let handle = spawn_orchestrator(&orchestrator).await;

    session_store
        .create_channel("research", "bus:research")
        .unwrap();

    let resp = send_and_recv(
        &bus,
        &mut resp_rx,
        dispatch_msg("r1", "!read research"),
    )
    .await;
    let text = notice_text(&resp);
    assert!(
        text.contains("research"),
        "Read should mention session name, got: {}",
        text
    );

    handle.abort();
}

#[tokio::test]
async fn test_dispatch_broadcast() {
    let bus = Arc::new(MessageBus::new(64));
    let (orchestrator, session_store, _tmp) = create_test_orchestrator(Arc::clone(&bus));
    let mut resp_rx = bus.subscribe_responses();
    let mut inbound_rx = bus.subscribe_inbound();
    let handle = spawn_orchestrator(&orchestrator).await;

    // Create two sessions
    session_store
        .create_channel("alpha", "bus:alpha")
        .unwrap();
    session_store.create_channel("beta", "bus:beta").unwrap();

    let resp = send_and_recv(
        &bus,
        &mut resp_rx,
        dispatch_msg("b1", "!broadcast hey everyone"),
    )
    .await;
    let text = notice_text(&resp);
    assert!(
        text.to_lowercase().contains("broadcast"),
        "Should confirm broadcast, got: {}",
        text
    );

    // Should find relayed messages on inbound bus targeting each session
    let mut targets_found = std::collections::HashSet::new();
    for _ in 0..20 {
        match timeout(Duration::from_millis(500), inbound_rx.recv()).await {
            Ok(Ok(msg)) => {
                if let SessionTarget::Session { name } = &msg.session_target {
                    if msg.body == "hey everyone" {
                        targets_found.insert(name.clone());
                    }
                }
            }
            _ => break,
        }
    }
    assert!(
        targets_found.contains("alpha"),
        "Broadcast should target 'alpha'"
    );
    assert!(
        targets_found.contains("beta"),
        "Broadcast should target 'beta'"
    );

    handle.abort();
}

#[tokio::test]
async fn test_agent_message_no_warm_manager() {
    let bus = Arc::new(MessageBus::new(64));
    let (orchestrator, session_store, _tmp) = create_test_orchestrator(Arc::clone(&bus));
    let mut resp_rx = bus.subscribe_responses();
    let handle = spawn_orchestrator(&orchestrator).await;

    // Create a session so there's a target
    session_store
        .create_channel("research", "bus:research")
        .unwrap();

    // Send a message targeting the session (not DISPATCH)
    let msg = BusMessage {
        id: "agent-1".to_string(),
        source: MessageSource::Web {
            connection_id: "test-conn-1".to_string(),
        },
        session_target: SessionTarget::Session {
            name: "research".to_string(),
        },
        sender: "test-user".to_string(),
        body: "summarize the paper".to_string(),
        timestamp: Utc::now(),
    };
    bus.publish_inbound(msg);

    let resp = timeout(Duration::from_secs(2), resp_rx.recv())
        .await
        .expect("timed out waiting for response")
        .expect("failed to receive response");

    assert_eq!(resp.session_name, "research");
    match &resp.content {
        ResponseContent::Error(text) => {
            assert!(
                text.to_lowercase().contains("no agent backend"),
                "Should say no agent backend, got: {}",
                text
            );
        }
        other => panic!(
            "Expected Error about no backend configured, got {:?}",
            other
        ),
    }

    handle.abort();
}

#[tokio::test]
async fn test_dispatch_create_duplicate_fails() {
    let bus = Arc::new(MessageBus::new(64));
    let (orchestrator, _store, _tmp) = create_test_orchestrator(Arc::clone(&bus));
    let mut resp_rx = bus.subscribe_responses();
    let handle = spawn_orchestrator(&orchestrator).await;

    // Create first
    let resp = send_and_recv(&bus, &mut resp_rx, dispatch_msg("dup1", "!create myproject")).await;
    let text = notice_text(&resp);
    assert!(text.to_lowercase().contains("created"));

    // Create duplicate
    let resp = send_and_recv(&bus, &mut resp_rx, dispatch_msg("dup2", "!create myproject")).await;
    let text = notice_text(&resp);
    assert!(
        text.to_lowercase().contains("failed"),
        "Duplicate create should fail, got: {}",
        text
    );

    handle.abort();
}
