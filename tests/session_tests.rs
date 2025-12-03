#[test]
fn test_session_create_and_load() {
    let temp_dir = std::env::temp_dir().join("matrix-bridge-test-sessions");
    let _ = std::fs::remove_dir_all(&temp_dir);

    let store = matrix_bridge::session::SessionStore::new(&temp_dir).unwrap();
    let room_id = "!test:example.com";

    // First load creates new session
    let session1 = store.get_or_create(room_id).unwrap();
    assert!(!session1.started);

    // Mark as started and save
    store.mark_started(room_id).unwrap();

    // Second load returns same session, marked as started
    let session2 = store.get_or_create(room_id).unwrap();
    assert_eq!(session1.session_id, session2.session_id);
    assert!(session2.started);

    // Cleanup
    std::fs::remove_dir_all(&temp_dir).unwrap();
}

#[test]
fn test_session_cli_args_first_message() {
    let session = matrix_bridge::session::Session {
        session_id: "test-uuid".to_string(),
        started: false,
    };

    let args = session.cli_args();

    assert_eq!(args, vec!["--session-id", "test-uuid"]);
}

#[test]
fn test_session_cli_args_continuation() {
    let session = matrix_bridge::session::Session {
        session_id: "test-uuid".to_string(),
        started: true,
    };

    let args = session.cli_args();

    assert_eq!(args, vec!["--resume", "test-uuid"]);
}
