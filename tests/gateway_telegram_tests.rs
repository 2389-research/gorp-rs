// ABOUTME: Tests for Telegram gateway adapter type conversions.
// ABOUTME: Validates ResponseContent to Telegram message and Telegram event to BusMessage conversions.

#[cfg(feature = "telegram")]
mod tests {
    use gorp::bus::*;
    use gorp::gateway::telegram::{response_to_telegram_message, telegram_event_to_bus_message};

    // =========================================================================
    // response_to_telegram_message tests
    // =========================================================================

    #[test]
    fn test_response_chunk_to_telegram_message() {
        let content = ResponseContent::Chunk("hello **world**".to_string());
        let msg = response_to_telegram_message(&content);
        assert!(msg.contains("hello"));
        assert!(msg.contains("world"));
    }

    #[test]
    fn test_response_complete_to_telegram_message() {
        let content = ResponseContent::Complete("final answer".to_string());
        let msg = response_to_telegram_message(&content);
        assert!(msg.contains("final answer"));
    }

    #[test]
    fn test_response_error_to_telegram_message() {
        let content = ResponseContent::Error("timeout".to_string());
        let msg = response_to_telegram_message(&content);
        assert!(msg.contains("Error"));
        assert!(msg.contains("timeout"));
    }

    #[test]
    fn test_response_system_notice_to_telegram_message() {
        let content = ResponseContent::SystemNotice("Session created".to_string());
        let msg = response_to_telegram_message(&content);
        assert!(msg.contains("Session created"));
    }

    #[test]
    fn test_response_chunk_empty_string() {
        let content = ResponseContent::Chunk(String::new());
        let msg = response_to_telegram_message(&content);
        assert!(msg.is_empty());
    }

    #[test]
    fn test_response_complete_preserves_multiline() {
        let content = ResponseContent::Complete("line 1\nline 2\n\n  indented".to_string());
        let msg = response_to_telegram_message(&content);
        assert!(msg.contains("line 1\nline 2"));
        assert!(msg.contains("  indented"));
    }

    // =========================================================================
    // telegram_event_to_bus_message tests
    // =========================================================================

    #[test]
    fn test_telegram_event_to_bus_message_dispatch() {
        let msg = telegram_event_to_bus_message(
            "12345",
            "100",
            "987654",
            "hello there",
            SessionTarget::Dispatch,
        );
        assert_eq!(msg.id, "100");
        assert_eq!(msg.sender, "987654");
        assert_eq!(msg.body, "hello there");
        assert!(matches!(
            msg.source,
            MessageSource::Platform {
                ref platform_id,
                ref channel_id
            } if platform_id == "telegram" && channel_id == "12345"
        ));
        assert!(matches!(msg.session_target, SessionTarget::Dispatch));
    }

    #[test]
    fn test_telegram_event_to_bus_message_session_target() {
        let msg = telegram_event_to_bus_message(
            "12345",
            "101",
            "987654",
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
    fn test_telegram_event_to_bus_message_preserves_body_verbatim() {
        let body = "line 1\nline 2\n\n  indented";
        let msg = telegram_event_to_bus_message(
            "12345",
            "102",
            "987654",
            body,
            SessionTarget::Dispatch,
        );
        assert_eq!(msg.body, body);
    }

    #[test]
    fn test_telegram_event_to_bus_message_timestamp_is_recent() {
        let before = chrono::Utc::now();
        let msg = telegram_event_to_bus_message(
            "12345",
            "103",
            "987654",
            "test",
            SessionTarget::Dispatch,
        );
        let after = chrono::Utc::now();
        assert!(msg.timestamp >= before);
        assert!(msg.timestamp <= after);
    }

    #[test]
    fn test_telegram_event_to_bus_message_negative_chat_id() {
        let msg = telegram_event_to_bus_message(
            "-100123456789",
            "104",
            "987654",
            "group message",
            SessionTarget::Dispatch,
        );
        assert!(matches!(
            msg.source,
            MessageSource::Platform {
                ref channel_id,
                ..
            } if channel_id == "-100123456789"
        ));
    }
}
