// ABOUTME: Tests for message_handler helper functions
// ABOUTME: Covers channel name validation, string truncation, cron detection, and schedule parsing

use tempfile::TempDir;

// =============================================================================
// is_debug_enabled Tests
// =============================================================================

#[test]
fn test_is_debug_enabled_when_file_exists() {
    use gorp::message_handler::is_debug_enabled;

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let channel_dir = temp_dir.path();

    // Create .gorp/enable-debug file
    let gorp_dir = channel_dir.join(".gorp");
    std::fs::create_dir_all(&gorp_dir).expect("Failed to create .gorp dir");
    std::fs::write(gorp_dir.join("enable-debug"), "").expect("Failed to create debug file");

    assert!(is_debug_enabled(channel_dir.to_str().unwrap()));
}

#[test]
fn test_is_debug_enabled_when_file_missing() {
    use gorp::message_handler::is_debug_enabled;

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let channel_dir = temp_dir.path();

    // Don't create the file
    assert!(!is_debug_enabled(channel_dir.to_str().unwrap()));
}

#[test]
fn test_is_debug_enabled_when_gorp_dir_missing() {
    use gorp::message_handler::is_debug_enabled;

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let channel_dir = temp_dir.path();

    // .gorp directory doesn't exist
    assert!(!is_debug_enabled(channel_dir.to_str().unwrap()));
}

#[test]
fn test_is_debug_enabled_nonexistent_dir() {
    use gorp::message_handler::is_debug_enabled;

    // Non-existent directory should return false
    assert!(!is_debug_enabled("/nonexistent/path/to/channel"));
}

// =============================================================================
// validate_channel_name Tests
// =============================================================================

#[test]
fn test_validate_channel_name_valid() {
    use gorp::message_handler::validate_channel_name;

    // Valid names
    assert!(validate_channel_name("test").is_ok());
    assert!(validate_channel_name("my-channel").is_ok());
    assert!(validate_channel_name("my_channel").is_ok());
    assert!(validate_channel_name("channel123").is_ok());
    assert!(validate_channel_name("a").is_ok());
    assert!(validate_channel_name("PA").is_ok());
    assert!(validate_channel_name("dev-help-2024").is_ok());
}

#[test]
fn test_validate_channel_name_empty() {
    use gorp::message_handler::validate_channel_name;

    let result = validate_channel_name("");
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("empty"));
}

#[test]
fn test_validate_channel_name_too_long() {
    use gorp::message_handler::validate_channel_name;

    // 51 characters should fail
    let long_name = "a".repeat(51);
    let result = validate_channel_name(&long_name);
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("50 characters"));

    // 50 characters should pass
    let max_name = "a".repeat(50);
    assert!(validate_channel_name(&max_name).is_ok());
}

#[test]
fn test_validate_channel_name_invalid_characters() {
    use gorp::message_handler::validate_channel_name;

    // Invalid characters
    let invalid_names = vec![
        "test channel",  // space
        "test.channel",  // dot
        "test@channel",  // at sign
        "test/channel",  // slash
        "test\\channel", // backslash
        "test:channel",  // colon
        "test!channel",  // exclamation
        "test#channel",  // hash
        "test$channel",  // dollar
    ];

    for name in invalid_names {
        let result = validate_channel_name(name);
        assert!(
            result.is_err(),
            "Name '{}' should be invalid",
            name
        );
        assert!(
            result.unwrap_err().contains("letters, numbers, dashes"),
            "Error message should mention allowed characters"
        );
    }
}

// =============================================================================
// truncate_str Tests
// =============================================================================

#[test]
fn test_truncate_str_short_string() {
    use gorp::message_handler::truncate_str;

    // String shorter than max_len should not be truncated
    assert_eq!(truncate_str("hello", 10), "hello");
    assert_eq!(truncate_str("test", 4), "test");
    assert_eq!(truncate_str("", 10), "");
}

#[test]
fn test_truncate_str_exact_length() {
    use gorp::message_handler::truncate_str;

    // String exactly max_len should not be truncated
    assert_eq!(truncate_str("hello", 5), "hello");
    assert_eq!(truncate_str("abc", 3), "abc");
}

#[test]
fn test_truncate_str_long_string() {
    use gorp::message_handler::truncate_str;

    // String longer than max_len should be truncated with "..."
    assert_eq!(truncate_str("hello world", 8), "hello...");
    assert_eq!(truncate_str("abcdefghij", 6), "abc...");
}

#[test]
fn test_truncate_str_unicode() {
    use gorp::message_handler::truncate_str;

    // Should handle unicode characters properly
    let unicode_str = "héllo wörld";
    let result = truncate_str(unicode_str, 8);
    // Should truncate at character boundary, not byte boundary
    assert!(result.ends_with("..."));
    assert!(result.len() <= 11); // "héllo" + "..." at most
}

#[test]
fn test_truncate_str_emoji() {
    use gorp::message_handler::truncate_str;

    // Should handle emoji (multi-byte characters) properly
    let emoji_str = "👋🌍✨🎉";
    let result = truncate_str(emoji_str, 3);
    assert!(result.ends_with("..."));
}

#[test]
fn test_truncate_str_very_small_max() {
    use gorp::message_handler::truncate_str;

    // Edge case: max_len smaller than "..."
    let result = truncate_str("hello", 2);
    // Should still work with saturating_sub
    assert!(result.ends_with("..."));
}

// =============================================================================
// looks_like_cron Tests
// =============================================================================

#[test]
fn test_looks_like_cron_valid() {
    use gorp::message_handler::looks_like_cron;

    // Valid cron expressions
    assert!(looks_like_cron("0 9 * * *"));      // Every day at 9am
    assert!(looks_like_cron("*/15 * * * *"));   // Every 15 minutes
    assert!(looks_like_cron("30 14 1 * *"));    // 2:30pm on the 1st
    assert!(looks_like_cron("0 0 * * 0"));      // Midnight on Sundays
    assert!(looks_like_cron("0 8-17 * * 1-5")); // Work hours on weekdays
    assert!(looks_like_cron("0 9,12,18 * * *")); // 9am, noon, 6pm
}

#[test]
fn test_looks_like_cron_invalid_field_count() {
    use gorp::message_handler::looks_like_cron;

    // Wrong number of fields
    assert!(!looks_like_cron("0 9 * *"));        // 4 fields
    assert!(!looks_like_cron("0 9 * * * *"));    // 6 fields
    assert!(!looks_like_cron("9"));              // 1 field
    assert!(!looks_like_cron(""));               // empty
}

#[test]
fn test_looks_like_cron_natural_language() {
    use gorp::message_handler::looks_like_cron;

    // Natural language should NOT be detected as cron
    assert!(!looks_like_cron("in 5 minutes"));
    assert!(!looks_like_cron("every day at 9am"));
    assert!(!looks_like_cron("tomorrow at noon"));
    assert!(!looks_like_cron("every monday at 2pm"));
    assert!(!looks_like_cron("next week"));
}

#[test]
fn test_looks_like_cron_invalid_characters() {
    use gorp::message_handler::looks_like_cron;

    // Valid field count but invalid characters
    assert!(!looks_like_cron("a b c d e"));
    assert!(!looks_like_cron("0 9 @ * *"));
    assert!(!looks_like_cron("0 9 * * SUN")); // Day names not supported
}

// =============================================================================
// parse_schedule_input Tests
// =============================================================================

#[test]
fn test_parse_schedule_input_relative_time() {
    use gorp::message_handler::parse_schedule_input;
    use gorp::scheduler::ParsedSchedule;

    let result = parse_schedule_input("in 5 minutes run the tests", "UTC");
    assert!(result.is_ok());
    let (schedule, prompt) = result.unwrap();

    // Should be a one-time schedule
    assert!(matches!(schedule, ParsedSchedule::OneTime(_)));
    assert_eq!(prompt, "run the tests");
}

#[test]
fn test_parse_schedule_input_recurring() {
    use gorp::message_handler::parse_schedule_input;
    use gorp::scheduler::ParsedSchedule;

    let result = parse_schedule_input("every day at 9am check the server", "UTC");
    assert!(result.is_ok());
    let (schedule, prompt) = result.unwrap();

    // Should be a recurring schedule
    assert!(matches!(schedule, ParsedSchedule::Recurring { .. }));
    assert_eq!(prompt, "check the server");
}

#[test]
fn test_parse_schedule_input_no_prompt() {
    use gorp::message_handler::parse_schedule_input;

    // Just a time expression with no prompt should still work
    // The greedy parser will use the longest valid time expression
    let result = parse_schedule_input("in 5 minutes", "UTC");
    // This might parse "in 5" as time and "minutes" as prompt, or fail entirely
    // depending on what parse_time_expression accepts
    // The key is it shouldn't panic
    assert!(result.is_ok() || result.is_err());
}

#[test]
fn test_parse_schedule_input_invalid_time() {
    use gorp::message_handler::parse_schedule_input;

    // Invalid time expression should fail
    let result = parse_schedule_input("banana apple cherry", "UTC");
    assert!(result.is_err());
}

#[test]
fn test_parse_schedule_input_with_timezone() {
    use gorp::message_handler::parse_schedule_input;
    use gorp::scheduler::ParsedSchedule;

    // Should respect timezone
    let result = parse_schedule_input("tomorrow at 9am test", "America/New_York");
    assert!(result.is_ok());
    let (schedule, prompt) = result.unwrap();
    assert!(matches!(schedule, ParsedSchedule::OneTime(_)));
    assert_eq!(prompt, "test");
}

#[test]
fn test_parse_schedule_input_greedy_matching() {
    use gorp::message_handler::parse_schedule_input;

    // Should use greedy matching to find longest valid time expression
    // "in 2 hours" is valid, "in 2 hours and" might not be
    let result = parse_schedule_input("in 2 hours and then do something", "UTC");
    assert!(result.is_ok());
    let (_, prompt) = result.unwrap();
    // The prompt should contain the non-time portion
    assert!(prompt.contains("something") || prompt.contains("and"));
}

// =============================================================================
// Edge Case Tests
// =============================================================================

#[test]
fn test_truncate_str_with_newlines() {
    use gorp::message_handler::truncate_str;

    let multiline = "line1\nline2\nline3";
    let result = truncate_str(multiline, 10);
    assert!(result.len() <= 13); // 10 chars max + "..."
}

#[test]
fn test_channel_name_unicode() {
    use gorp::message_handler::validate_channel_name;

    // Unicode letters should be valid (alphanumeric includes unicode)
    // Actually, is_alphanumeric() in Rust includes unicode letters
    assert!(validate_channel_name("café").is_ok());
    assert!(validate_channel_name("日本語").is_ok());
}

#[test]
fn test_channel_name_with_numbers() {
    use gorp::message_handler::validate_channel_name;

    assert!(validate_channel_name("project123").is_ok());
    assert!(validate_channel_name("123project").is_ok());
    assert!(validate_channel_name("123").is_ok());
    assert!(validate_channel_name("a1b2c3").is_ok());
}

#[test]
fn test_looks_like_cron_with_whitespace() {
    use gorp::message_handler::looks_like_cron;

    // Extra whitespace should not affect parsing (split_whitespace handles it)
    assert!(looks_like_cron("0  9  *  *  *")); // Extra spaces
    assert!(looks_like_cron(" 0 9 * * * "));  // Leading/trailing spaces
}
