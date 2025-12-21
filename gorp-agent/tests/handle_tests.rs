use gorp_agent::handle::{AgentHandle, EventReceiver};
use gorp_agent::AgentEvent;
use tokio::sync::mpsc;

fn assert_send<T: Send>() {}
fn assert_sync<T: Sync>() {}

#[test]
fn test_agent_handle_is_send_sync() {
    assert_send::<AgentHandle>();
    assert_sync::<AgentHandle>();
}

#[test]
fn test_event_receiver_is_send() {
    assert_send::<EventReceiver>();
}

#[tokio::test]
async fn test_event_receiver_receives_events() {
    let (tx, rx) = mpsc::channel(32);
    let mut receiver = EventReceiver::new(rx);

    tx.send(AgentEvent::Text("hello".to_string()))
        .await
        .unwrap();
    tx.send(AgentEvent::Text("world".to_string()))
        .await
        .unwrap();
    drop(tx);

    let event1 = receiver.recv().await.unwrap();
    assert!(matches!(event1, AgentEvent::Text(s) if s == "hello"));

    let event2 = receiver.recv().await.unwrap();
    assert!(matches!(event2, AgentEvent::Text(s) if s == "world"));

    let event3 = receiver.recv().await;
    assert!(event3.is_none());
}
