// ABOUTME: Tests for Matrix gateway adapter type conversions.
// ABOUTME: Validates ResponseContent to Matrix message and Matrix event to BusMessage conversions.

#[cfg(feature = "matrix")]
mod tests {
    use gorp::bus::*;
    use gorp::gateway::matrix::{matrix_event_to_bus_message, response_to_matrix_message};
    use matrix_sdk::ruma::events::room::message::MessageType;

    #[test]
    fn test_response_chunk_to_matrix_message() {
        let content = ResponseContent::Chunk("hello **world**".to_string());
        let msg = response_to_matrix_message(&content);
        match &msg.msgtype {
            MessageType::Text(text) => {
                assert!(text.body.contains("hello"));
                assert!(text.body.contains("**world**"));
                // Should have HTML formatted version
                assert!(text.formatted.is_some());
            }
            other => panic!("Expected Text message type, got {:?}", other),
        }
    }

    #[test]
    fn test_response_complete_to_matrix_message() {
        let content = ResponseContent::Complete("final answer".to_string());
        let msg = response_to_matrix_message(&content);
        match &msg.msgtype {
            MessageType::Text(text) => {
                assert!(text.body.contains("final"));
                assert!(text.formatted.is_some());
            }
            other => panic!("Expected Text message type, got {:?}", other),
        }
    }

    #[test]
    fn test_response_error_to_matrix_message() {
        let content = ResponseContent::Error("timeout".to_string());
        let msg = response_to_matrix_message(&content);
        match &msg.msgtype {
            MessageType::Text(text) => {
                assert!(text.body.contains("Error"));
                assert!(text.body.contains("timeout"));
                assert!(text.formatted.is_some());
                let html = text.formatted.as_ref().unwrap().body.clone();
                assert!(html.contains("<strong>"));
            }
            other => panic!("Expected Text message type, got {:?}", other),
        }
    }

    #[test]
    fn test_response_system_notice_to_matrix_message() {
        let content = ResponseContent::SystemNotice("Session created".to_string());
        let msg = response_to_matrix_message(&content);
        match &msg.msgtype {
            MessageType::Text(text) => {
                assert!(text.body.contains("Session created"));
                assert!(text.formatted.is_some());
                let html = text.formatted.as_ref().unwrap().body.clone();
                assert!(html.contains("<em>"));
            }
            other => panic!("Expected Text message type, got {:?}", other),
        }
    }

    #[test]
    fn test_matrix_event_to_bus_message() {
        let msg = matrix_event_to_bus_message(
            "!room123:matrix.org",
            "$event456",
            "@harper:matrix.org",
            "hello there",
            SessionTarget::Dispatch,
        );
        assert_eq!(msg.id, "$event456");
        assert_eq!(msg.sender, "@harper:matrix.org");
        assert_eq!(msg.body, "hello there");
        assert!(matches!(
            msg.source,
            MessageSource::Platform {
                ref platform_id,
                ref channel_id
            } if platform_id == "matrix" && channel_id == "!room123:matrix.org"
        ));
        assert!(matches!(msg.session_target, SessionTarget::Dispatch));
    }

    #[test]
    fn test_matrix_event_to_bus_message_session_target() {
        let msg = matrix_event_to_bus_message(
            "!room789:matrix.org",
            "$event101",
            "@alice:matrix.org",
            "summarize paper",
            SessionTarget::Session {
                name: "research".to_string(),
            },
        );
        assert!(
            matches!(msg.session_target, SessionTarget::Session { ref name } if name == "research")
        );
    }

    #[test]
    fn test_matrix_event_to_bus_message_preserves_body_verbatim() {
        let body = "line 1\nline 2\n\n  indented";
        let msg = matrix_event_to_bus_message(
            "!room:example.com",
            "$evt",
            "@user:example.com",
            body,
            SessionTarget::Dispatch,
        );
        assert_eq!(msg.body, body);
    }

    #[test]
    fn test_matrix_event_to_bus_message_timestamp_is_recent() {
        let before = chrono::Utc::now();
        let msg = matrix_event_to_bus_message(
            "!room:example.com",
            "$evt",
            "@user:example.com",
            "test",
            SessionTarget::Dispatch,
        );
        let after = chrono::Utc::now();
        assert!(msg.timestamp >= before);
        assert!(msg.timestamp <= after);
    }

    #[test]
    fn test_response_chunk_markdown_rendered_as_html() {
        let content = ResponseContent::Chunk("# Heading\n\n- item 1\n- item 2".to_string());
        let msg = response_to_matrix_message(&content);
        match &msg.msgtype {
            MessageType::Text(text) => {
                let html = text.formatted.as_ref().unwrap().body.clone();
                assert!(html.contains("<h1>") || html.contains("<h1"));
                assert!(html.contains("<li>"));
            }
            other => panic!("Expected Text message type, got {:?}", other),
        }
    }
}
