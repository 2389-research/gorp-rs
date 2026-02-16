// ABOUTME: Tests for Slack gateway adapter type conversions.
// ABOUTME: Validates ResponseContent to Slack message and Slack event to BusMessage conversions.

#[cfg(feature = "slack")]
mod tests {
    use gorp::bus::*;
    use gorp::gateway::slack::{response_to_slack_message, slack_event_to_bus_message};

    // =========================================================================
    // response_to_slack_message tests
    // =========================================================================

    #[test]
    fn test_response_chunk_to_slack_message() {
        let content = ResponseContent::Chunk("hello **world**".to_string());
        let msg = response_to_slack_message(&content);
        assert!(msg.contains("hello"));
        assert!(msg.contains("world"));
    }

    #[test]
    fn test_response_complete_to_slack_message() {
        let content = ResponseContent::Complete("final answer".to_string());
        let msg = response_to_slack_message(&content);
        assert!(msg.contains("final answer"));
    }

    #[test]
    fn test_response_error_to_slack_message() {
        let content = ResponseContent::Error("timeout".to_string());
        let msg = response_to_slack_message(&content);
        assert!(msg.contains("Error"));
        assert!(msg.contains("timeout"));
    }

    #[test]
    fn test_response_system_notice_to_slack_message() {
        let content = ResponseContent::SystemNotice("Session created".to_string());
        let msg = response_to_slack_message(&content);
        assert!(msg.contains("Session created"));
    }

    #[test]
    fn test_response_chunk_empty_string() {
        let content = ResponseContent::Chunk(String::new());
        let msg = response_to_slack_message(&content);
        assert!(msg.is_empty());
    }

    #[test]
    fn test_response_complete_preserves_multiline() {
        let content = ResponseContent::Complete("line 1\nline 2\n\n  indented".to_string());
        let msg = response_to_slack_message(&content);
        assert!(msg.contains("line 1\nline 2"));
        assert!(msg.contains("  indented"));
    }

    // =========================================================================
    // slack_event_to_bus_message tests
    // =========================================================================

    #[test]
    fn test_slack_event_to_bus_message_dispatch() {
        let msg = slack_event_to_bus_message(
            "C12345ABC",
            "1700000000.000100",
            "U98765XYZ",
            "hello there",
            SessionTarget::Dispatch,
        );
        assert_eq!(msg.id, "1700000000.000100");
        assert_eq!(msg.sender, "U98765XYZ");
        assert_eq!(msg.body, "hello there");
        assert!(matches!(
            msg.source,
            MessageSource::Platform {
                ref platform_id,
                ref channel_id
            } if platform_id == "slack" && channel_id == "C12345ABC"
        ));
        assert!(matches!(msg.session_target, SessionTarget::Dispatch));
    }

    #[test]
    fn test_slack_event_to_bus_message_session_target() {
        let msg = slack_event_to_bus_message(
            "C12345ABC",
            "1700000000.000200",
            "U98765XYZ",
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
    fn test_slack_event_to_bus_message_preserves_body_verbatim() {
        let body = "line 1\nline 2\n\n  indented";
        let msg = slack_event_to_bus_message(
            "C12345",
            "evt123",
            "U123",
            body,
            SessionTarget::Dispatch,
        );
        assert_eq!(msg.body, body);
    }

    #[test]
    fn test_slack_event_to_bus_message_timestamp_is_recent() {
        let before = chrono::Utc::now();
        let msg = slack_event_to_bus_message(
            "C12345",
            "evt123",
            "U123",
            "test",
            SessionTarget::Dispatch,
        );
        let after = chrono::Utc::now();
        assert!(msg.timestamp >= before);
        assert!(msg.timestamp <= after);
    }

    #[test]
    fn test_slack_event_to_bus_message_dm_channel() {
        let msg = slack_event_to_bus_message(
            "D98765",
            "evt_dm",
            "U123",
            "private message",
            SessionTarget::Dispatch,
        );
        assert!(matches!(
            msg.source,
            MessageSource::Platform {
                ref channel_id,
                ..
            } if channel_id == "D98765"
        ));
    }
}
