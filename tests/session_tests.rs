// ABOUTME: Tests for session/channel management
// ABOUTME: Verifies channel creation, persistence, and CLI argument generation

#[test]
fn test_channel_create_and_load() {
    let temp_dir = std::env::temp_dir().join("gorp-test-sessions");
    let _ = std::fs::remove_dir_all(&temp_dir);
    std::fs::create_dir_all(&temp_dir).unwrap();

    // Create template directory (required by SessionStore)
    let template_dir = temp_dir.join("template");
    std::fs::create_dir_all(&template_dir).unwrap();

    let store = gorp::session::SessionStore::new(&temp_dir).unwrap();
    let room_id = "!test:example.com";
    let channel_name = "test-channel";

    // Create a new channel
    let channel1 = store.create_channel(channel_name, room_id).unwrap();
    assert!(!channel1.started);
    assert_eq!(channel1.channel_name, channel_name);
    assert_eq!(channel1.room_id, room_id);

    // Mark as started
    store.mark_started(room_id).unwrap();

    // Load channel by room_id - should now be marked as started
    let channel2 = store.get_by_room(room_id).unwrap().unwrap();
    assert_eq!(channel1.session_id, channel2.session_id);
    assert!(channel2.started);

    // Load channel by name
    let channel3 = store.get_by_name(channel_name).unwrap().unwrap();
    assert_eq!(channel1.session_id, channel3.session_id);

    // Cleanup
    std::fs::remove_dir_all(&temp_dir).unwrap();
}

#[test]
fn test_channel_cli_args_first_message() {
    let channel = gorp::session::Channel {
        channel_name: "test".to_string(),
        room_id: "!test:example.com".to_string(),
        session_id: "test-uuid".to_string(),
        directory: "./workspace/test".to_string(),
        started: false,
        created_at: "2024-01-01T00:00:00Z".to_string(),
        backend_type: None,
    };

    let args = channel.cli_args();
    assert_eq!(args, vec!["--session-id", "test-uuid"]);
}

#[test]
fn test_channel_cli_args_continuation() {
    let channel = gorp::session::Channel {
        channel_name: "test".to_string(),
        room_id: "!test:example.com".to_string(),
        session_id: "test-uuid".to_string(),
        directory: "./workspace/test".to_string(),
        started: true,
        created_at: "2024-01-01T00:00:00Z".to_string(),
        backend_type: None,
    };

    let args = channel.cli_args();
    assert_eq!(args, vec!["--resume", "test-uuid"]);
}

#[test]
fn test_channel_validate_directory_rejects_traversal() {
    let channel = gorp::session::Channel {
        channel_name: "evil".to_string(),
        room_id: "!evil:example.com".to_string(),
        session_id: "evil-uuid".to_string(),
        directory: "../../../etc/passwd".to_string(),
        started: false,
        created_at: "2024-01-01T00:00:00Z".to_string(),
        backend_type: None,
    };

    assert!(channel.validate_directory().is_err());
}

#[test]
fn test_channel_validate_directory_accepts_valid() {
    let channel = gorp::session::Channel {
        channel_name: "good".to_string(),
        room_id: "!good:example.com".to_string(),
        session_id: "good-uuid".to_string(),
        directory: "./workspace/good".to_string(),
        started: false,
        created_at: "2024-01-01T00:00:00Z".to_string(),
        backend_type: None,
    };

    assert!(channel.validate_directory().is_ok());
}
