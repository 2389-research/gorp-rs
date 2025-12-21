use gorp_agent::backends::mock::MockBackend;
use gorp_agent::testing::recording::{RecordingAgent, ReplayAgent};
use gorp_agent::AgentEvent;
use serde_json::json;
use tempfile::TempDir;

#[tokio::test]
async fn test_recording_agent_wraps_handle() {
    let mock = MockBackend::new()
        .on_prompt("hello")
        .respond_text("Hi there!");

    let handle = mock.into_handle();
    let recording = RecordingAgent::wrap(handle);

    let session_id = recording.new_session().await.unwrap();
    let mut receiver = recording.prompt(&session_id, "hello").await.unwrap();

    let event = receiver.recv().await.unwrap();
    match event {
        AgentEvent::Result { text, .. } => assert_eq!(text, "Hi there!"),
        _ => panic!("Expected Result event, got {:?}", event),
    }
}

#[tokio::test]
async fn test_recording_agent_records_interactions() {
    let mock = MockBackend::new()
        .on_prompt("hello")
        .respond_text("Hi there!");

    let handle = mock.into_handle();
    let recording = RecordingAgent::wrap(handle);

    let session_id = recording.new_session().await.unwrap();
    let mut receiver = recording.prompt(&session_id, "hello").await.unwrap();

    // Consume all events
    while receiver.recv().await.is_some() {}

    // Check transcript was recorded
    let transcript = recording.transcript();
    assert_eq!(transcript.len(), 1);
    assert_eq!(transcript[0].prompt, "hello");
    assert_eq!(transcript[0].session_id, session_id);
    assert_eq!(transcript[0].events.len(), 1);
    match &transcript[0].events[0] {
        AgentEvent::Result { text, .. } => assert_eq!(text, "Hi there!"),
        _ => panic!("Expected Result event"),
    }
}

#[tokio::test]
async fn test_recording_agent_records_multiple_interactions() {
    let mock = MockBackend::new()
        .on_prompt("first")
        .respond_text("First response")
        .on_prompt("second")
        .respond_text("Second response");

    let handle = mock.into_handle();
    let recording = RecordingAgent::wrap(handle);

    let session_id = recording.new_session().await.unwrap();

    // First interaction
    let mut receiver = recording.prompt(&session_id, "first").await.unwrap();
    while receiver.recv().await.is_some() {}

    // Second interaction
    let mut receiver = recording.prompt(&session_id, "second").await.unwrap();
    while receiver.recv().await.is_some() {}

    // Check transcript
    let transcript = recording.transcript();
    assert_eq!(transcript.len(), 2);
    assert_eq!(transcript[0].prompt, "first");
    assert_eq!(transcript[1].prompt, "second");
}

#[tokio::test]
async fn test_recording_agent_records_tool_events() {
    let mock = MockBackend::new().on_prompt("read file").respond_with(vec![
        AgentEvent::ToolStart {
            id: "t1".to_string(),
            name: "Read".to_string(),
            input: json!({"path": "/tmp/foo"}),
        },
        AgentEvent::ToolEnd {
            id: "t1".to_string(),
            name: "Read".to_string(),
            output: json!({"content": "file contents"}),
            success: true,
            duration_ms: 10,
        },
        AgentEvent::Result {
            text: "Read the file".to_string(),
            usage: None,
            metadata: json!({}),
        },
    ]);

    let handle = mock.into_handle();
    let recording = RecordingAgent::wrap(handle);

    let session_id = recording.new_session().await.unwrap();
    let mut receiver = recording.prompt(&session_id, "read file").await.unwrap();

    while receiver.recv().await.is_some() {}

    let transcript = recording.transcript();
    assert_eq!(transcript.len(), 1);
    assert_eq!(transcript[0].events.len(), 3);
    assert!(
        matches!(&transcript[0].events[0], AgentEvent::ToolStart { name, .. } if name == "Read")
    );
    assert!(matches!(
        &transcript[0].events[1],
        AgentEvent::ToolEnd { success: true, .. }
    ));
    assert!(matches!(
        &transcript[0].events[2],
        AgentEvent::Result { .. }
    ));
}

#[tokio::test]
async fn test_recording_agent_save_load_transcript() {
    let temp_dir = TempDir::new().unwrap();
    let transcript_path = temp_dir.path().join("transcript.json");

    // Record interactions
    {
        let mock = MockBackend::new()
            .on_prompt("hello")
            .respond_text("Hi there!");

        let handle = mock.into_handle();
        let recording = RecordingAgent::wrap(handle);

        let session_id = recording.new_session().await.unwrap();
        let mut receiver = recording.prompt(&session_id, "hello").await.unwrap();
        while receiver.recv().await.is_some() {}

        // Save transcript
        recording.save_transcript(&transcript_path).await.unwrap();
    }

    // Load transcript
    let loaded = std::fs::read_to_string(&transcript_path).unwrap();
    let interactions: Vec<gorp_agent::testing::recording::Interaction> =
        serde_json::from_str(&loaded).unwrap();

    assert_eq!(interactions.len(), 1);
    assert_eq!(interactions[0].prompt, "hello");
}

#[tokio::test]
async fn test_recording_agent_into_parts() {
    let mock = MockBackend::new()
        .on_prompt("test")
        .respond_text("response");

    let handle = mock.into_handle();
    let recording = RecordingAgent::wrap(handle);

    let session_id = recording.new_session().await.unwrap();
    let mut receiver = recording.prompt(&session_id, "test").await.unwrap();
    while receiver.recv().await.is_some() {}

    let (_handle, transcript) = recording.into_parts();
    assert_eq!(transcript.len(), 1);
    assert_eq!(transcript[0].prompt, "test");
}

#[tokio::test]
async fn test_replay_agent_from_transcript() {
    // Create a transcript manually
    let interactions = vec![gorp_agent::testing::recording::Interaction {
        timestamp: std::time::SystemTime::now(),
        session_id: "replay-session-1".to_string(),
        prompt: "hello".to_string(),
        events: vec![AgentEvent::Result {
            text: "Hi there!".to_string(),
            usage: None,
            metadata: json!({}),
        }],
    }];

    let replay = ReplayAgent::from_transcript(interactions);
    let handle = replay.into_handle();

    let session_id = handle.new_session().await.unwrap();
    let mut receiver = handle.prompt(&session_id, "hello").await.unwrap();

    let event = receiver.recv().await.unwrap();
    match event {
        AgentEvent::Result { text, .. } => assert_eq!(text, "Hi there!"),
        _ => panic!("Expected Result event"),
    }
}

#[tokio::test]
async fn test_replay_agent_multiple_interactions() {
    let interactions = vec![
        gorp_agent::testing::recording::Interaction {
            timestamp: std::time::SystemTime::now(),
            session_id: "replay-session-1".to_string(),
            prompt: "first".to_string(),
            events: vec![AgentEvent::Result {
                text: "First response".to_string(),
                usage: None,
                metadata: json!({}),
            }],
        },
        gorp_agent::testing::recording::Interaction {
            timestamp: std::time::SystemTime::now(),
            session_id: "replay-session-1".to_string(),
            prompt: "second".to_string(),
            events: vec![AgentEvent::Result {
                text: "Second response".to_string(),
                usage: None,
                metadata: json!({}),
            }],
        },
    ];

    let replay = ReplayAgent::from_transcript(interactions);
    let handle = replay.into_handle();

    let session_id = handle.new_session().await.unwrap();

    // First prompt
    let mut receiver = handle.prompt(&session_id, "first").await.unwrap();
    let event = receiver.recv().await.unwrap();
    match event {
        AgentEvent::Result { text, .. } => assert_eq!(text, "First response"),
        _ => panic!("Expected Result event"),
    }

    // Second prompt
    let mut receiver = handle.prompt(&session_id, "second").await.unwrap();
    let event = receiver.recv().await.unwrap();
    match event {
        AgentEvent::Result { text, .. } => assert_eq!(text, "Second response"),
        _ => panic!("Expected Result event"),
    }
}

#[tokio::test]
async fn test_replay_agent_load_from_file() {
    let temp_dir = TempDir::new().unwrap();
    let transcript_path = temp_dir.path().join("transcript.json");

    // Create and save a transcript
    let interactions = vec![gorp_agent::testing::recording::Interaction {
        timestamp: std::time::SystemTime::now(),
        session_id: "replay-session-1".to_string(),
        prompt: "hello".to_string(),
        events: vec![AgentEvent::Result {
            text: "Hi there!".to_string(),
            usage: None,
            metadata: json!({}),
        }],
    }];

    std::fs::write(
        &transcript_path,
        serde_json::to_string_pretty(&interactions).unwrap(),
    )
    .unwrap();

    // Load and replay
    let replay = ReplayAgent::load(&transcript_path).await.unwrap();
    let handle = replay.into_handle();

    let session_id = handle.new_session().await.unwrap();
    let mut receiver = handle.prompt(&session_id, "hello").await.unwrap();

    let event = receiver.recv().await.unwrap();
    match event {
        AgentEvent::Result { text, .. } => assert_eq!(text, "Hi there!"),
        _ => panic!("Expected Result event"),
    }
}

#[tokio::test]
async fn test_record_and_replay_roundtrip() {
    let temp_dir = TempDir::new().unwrap();
    let transcript_path = temp_dir.path().join("transcript.json");

    // Step 1: Record interactions
    {
        let mock = MockBackend::new().on_prompt("test").respond_with(vec![
            AgentEvent::Text("Thinking...".to_string()),
            AgentEvent::Result {
                text: "Done!".to_string(),
                usage: None,
                metadata: json!({}),
            },
        ]);

        let handle = mock.into_handle();
        let recording = RecordingAgent::wrap(handle);

        let session_id = recording.new_session().await.unwrap();
        let mut receiver = recording.prompt(&session_id, "test").await.unwrap();
        while receiver.recv().await.is_some() {}

        recording.save_transcript(&transcript_path).await.unwrap();
    }

    // Step 2: Replay from saved file
    let replay = ReplayAgent::load(&transcript_path).await.unwrap();
    let handle = replay.into_handle();

    let session_id = handle.new_session().await.unwrap();
    let mut receiver = handle.prompt(&session_id, "test").await.unwrap();

    let mut events = vec![];
    while let Some(e) = receiver.recv().await {
        events.push(e);
    }

    assert_eq!(events.len(), 2);
    assert!(matches!(&events[0], AgentEvent::Text(s) if s == "Thinking..."));
    assert!(matches!(&events[1], AgentEvent::Result { text, .. } if text == "Done!"));
}
