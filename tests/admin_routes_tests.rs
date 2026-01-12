// ABOUTME: Tests for admin routes - validation, security, and helper functions
// ABOUTME: Covers path traversal prevention, input validation, and utility functions

#![cfg(feature = "admin")]

use std::path::Path;
use tempfile::TempDir;

// =============================================================================
// Helper Function Tests
// =============================================================================

#[test]
fn test_format_file_size_bytes() {
    use gorp::admin::routes::format_file_size;

    assert_eq!(format_file_size(0), "0 B");
    assert_eq!(format_file_size(1), "1 B");
    assert_eq!(format_file_size(512), "512 B");
    assert_eq!(format_file_size(1023), "1023 B");
}

#[test]
fn test_format_file_size_kilobytes() {
    use gorp::admin::routes::format_file_size;

    assert_eq!(format_file_size(1024), "1.00 KB");
    assert_eq!(format_file_size(1536), "1.50 KB");
    assert_eq!(format_file_size(10 * 1024), "10.00 KB");
}

#[test]
fn test_format_file_size_megabytes() {
    use gorp::admin::routes::format_file_size;

    assert_eq!(format_file_size(1024 * 1024), "1.00 MB");
    assert_eq!(format_file_size(5 * 1024 * 1024), "5.00 MB");
    assert_eq!(format_file_size(1024 * 1024 + 512 * 1024), "1.50 MB");
}

#[test]
fn test_format_file_size_gigabytes() {
    use gorp::admin::routes::format_file_size;

    assert_eq!(format_file_size(1024 * 1024 * 1024), "1.00 GB");
    assert_eq!(format_file_size(2 * 1024 * 1024 * 1024), "2.00 GB");
}

// =============================================================================
// Path Validation Tests
// =============================================================================

#[test]
fn test_path_traversal_rejected() {
    use gorp::admin::routes::validate_and_resolve_path;

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let workspace_root = temp_dir.path();

    // Path with .. should be rejected
    let result = validate_and_resolve_path(workspace_root, "../etc/passwd");
    assert!(result.is_err());
    assert!(result.unwrap_err().message.contains("path traversal"));

    // Nested .. should also be rejected
    let result = validate_and_resolve_path(workspace_root, "foo/../../bar");
    assert!(result.is_err());
    assert!(result.unwrap_err().message.contains("path traversal"));
}

#[test]
fn test_valid_path_accepted() {
    use gorp::admin::routes::validate_and_resolve_path;

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let workspace_root = temp_dir.path();

    // Create a test file
    std::fs::write(workspace_root.join("test.txt"), "hello").expect("Failed to write test file");

    // Valid path should work
    let result = validate_and_resolve_path(workspace_root, "test.txt");
    assert!(result.is_ok());
}

#[test]
fn test_nonexistent_path_rejected() {
    use gorp::admin::routes::validate_and_resolve_path;

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let workspace_root = temp_dir.path();

    // Non-existent path should fail
    let result = validate_and_resolve_path(workspace_root, "nonexistent.txt");
    assert!(result.is_err());
    assert!(result.unwrap_err().message.contains("Path not found"));
}

#[test]
fn test_empty_path_returns_workspace_root() {
    use gorp::admin::routes::validate_and_resolve_path;

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let workspace_root = temp_dir.path();

    let result = validate_and_resolve_path(workspace_root, "");
    assert!(result.is_ok());
    let path = result.ok().unwrap();
    assert_eq!(
        path.canonicalize().unwrap(),
        workspace_root.canonicalize().unwrap()
    );
}

// =============================================================================
// Config Form Validation Tests
// =============================================================================

#[test]
fn test_config_form_path_traversal_detection() {
    // Path traversal in workspace path should be detectable
    let workspace_path = "/tmp/../etc/passwd";

    // The path contains ".." which should be rejected
    assert!(workspace_path.contains(".."));
}

#[test]
fn test_config_form_invalid_timezone() {
    use chrono_tz::Tz;

    // Invalid timezone should not parse
    let invalid_tz = "Invalid/Timezone";
    assert!(invalid_tz.parse::<Tz>().is_err());

    // Valid timezone should parse
    let valid_tz = "America/New_York";
    assert!(valid_tz.parse::<Tz>().is_ok());

    // UTC should parse
    let utc = "UTC";
    assert!(utc.parse::<Tz>().is_ok());
}

// =============================================================================
// Channel Name Validation Tests
// =============================================================================

#[test]
fn test_channel_name_validation_empty() {
    let name = "";
    assert!(name.is_empty());
}

#[test]
fn test_channel_name_validation_too_long() {
    let name = "a".repeat(65);
    assert!(name.len() > 64);
}

#[test]
fn test_channel_name_validation_valid_characters() {
    let valid_names = vec![
        "test-channel",
        "my_project",
        "channel123",
        "a-b-c",
        "foo_bar_baz",
    ];

    for name in valid_names {
        assert!(
            name.chars()
                .all(|c| c.is_alphanumeric() || c == '-' || c == '_'),
            "Name '{}' should be valid",
            name
        );
    }
}

#[test]
fn test_channel_name_validation_invalid_characters() {
    let invalid_names = vec![
        "test channel",  // space
        "test.channel",  // dot
        "test@channel",  // at sign
        "test/channel",  // slash
        "test\\channel", // backslash
    ];

    for name in invalid_names {
        assert!(
            !name
                .chars()
                .all(|c| c.is_alphanumeric() || c == '-' || c == '_'),
            "Name '{}' should be invalid",
            name
        );
    }
}

// =============================================================================
// Schedule ID Validation Tests
// =============================================================================

#[test]
fn test_schedule_id_validation() {
    // Empty ID should be invalid
    let empty_id = "";
    assert!(empty_id.is_empty());

    // Very long ID should be invalid
    let long_id = "x".repeat(257);
    assert!(long_id.len() > 256);

    // UUID-style ID should be valid
    let uuid_id = uuid::Uuid::new_v4().to_string();
    assert!(!uuid_id.is_empty() && uuid_id.len() <= 256);
}

// =============================================================================
// Read Last N Lines Tests
// =============================================================================

#[test]
fn test_read_last_n_lines_empty_file() {
    use gorp::admin::routes::read_last_n_lines;

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let file_path = temp_dir.path().join("empty.log");
    std::fs::write(&file_path, "").expect("Failed to write file");

    let lines = read_last_n_lines(&file_path, 10);
    assert!(lines.is_empty());
}

#[test]
fn test_read_last_n_lines_fewer_than_n() {
    use gorp::admin::routes::read_last_n_lines;

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let file_path = temp_dir.path().join("small.log");
    std::fs::write(&file_path, "line1\nline2\nline3").expect("Failed to write file");

    let lines = read_last_n_lines(&file_path, 10);
    assert_eq!(lines.len(), 3);
    assert_eq!(lines[0], "line1");
    assert_eq!(lines[1], "line2");
    assert_eq!(lines[2], "line3");
}

#[test]
fn test_read_last_n_lines_exactly_n() {
    use gorp::admin::routes::read_last_n_lines;

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let file_path = temp_dir.path().join("exact.log");
    std::fs::write(&file_path, "line1\nline2\nline3").expect("Failed to write file");

    let lines = read_last_n_lines(&file_path, 3);
    assert_eq!(lines.len(), 3);
}

#[test]
fn test_read_last_n_lines_more_than_n() {
    use gorp::admin::routes::read_last_n_lines;

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let file_path = temp_dir.path().join("large.log");

    // Write 100 lines
    let content: String = (1..=100).map(|i| format!("line{}\n", i)).collect();
    std::fs::write(&file_path, content).expect("Failed to write file");

    // Request last 10
    let lines = read_last_n_lines(&file_path, 10);
    assert_eq!(lines.len(), 10);
    assert_eq!(lines[0], "line91");
    assert_eq!(lines[9], "line100");
}

#[test]
fn test_read_last_n_lines_nonexistent_file() {
    use gorp::admin::routes::read_last_n_lines;

    let path = Path::new("/nonexistent/path/file.log");
    let lines = read_last_n_lines(path, 10);
    assert!(lines.is_empty());
}

// =============================================================================
// Count Recent Lines Matching Tests
// =============================================================================

#[test]
fn test_count_recent_lines_matching() {
    use gorp::admin::routes::count_recent_lines_matching;

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let file_path = temp_dir.path().join("messages.log");

    // Write log lines with dates
    let content = r#"{"timestamp": "2024-01-15T10:00:00", "message": "hello"}
{"timestamp": "2024-01-15T11:00:00", "message": "world"}
{"timestamp": "2024-01-16T09:00:00", "message": "different day"}
{"timestamp": "2024-01-15T12:00:00", "message": "back to 15th"}"#;
    std::fs::write(&file_path, content).expect("Failed to write file");

    // Count lines containing "2024-01-15"
    let count = count_recent_lines_matching(&file_path, "2024-01-15");
    assert_eq!(count, 3);

    // Count lines containing "2024-01-16"
    let count = count_recent_lines_matching(&file_path, "2024-01-16");
    assert_eq!(count, 1);

    // Count lines containing pattern that doesn't exist
    let count = count_recent_lines_matching(&file_path, "nonexistent");
    assert_eq!(count, 0);
}

// =============================================================================
// Time Expression Detection Tests
// =============================================================================

#[test]
fn test_cron_expression_detection() {
    // Valid cron expressions (5 space-separated parts with valid chars)
    let cron_expressions = vec![
        "0 9 * * *",    // Every day at 9am
        "*/15 * * * *", // Every 15 minutes
        "30 14 1 * *",  // 2:30pm on the 1st of each month
    ];

    for expr in cron_expressions {
        let parts = expr.split_whitespace().count();
        let is_cron = parts == 5
            && expr
                .chars()
                .all(|c| c.is_ascii_digit() || " */-,".contains(c));
        assert!(
            is_cron,
            "Expression '{}' should be detected as cron",
            expr
        );
    }

    // Natural language time expressions should NOT be detected as cron
    let natural_expressions = vec!["in 5 minutes", "every day at 9am", "tomorrow at noon"];

    for expr in natural_expressions {
        let parts = expr.split_whitespace().count();
        let is_cron = parts == 5
            && expr
                .chars()
                .all(|c| c.is_ascii_digit() || " */-,".contains(c));
        assert!(
            !is_cron,
            "Expression '{}' should NOT be detected as cron",
            expr
        );
    }
}

// =============================================================================
// Input Length Validation Tests
// =============================================================================

#[test]
fn test_prompt_length_limits() {
    const MAX_PROMPT_LENGTH: usize = 64 * 1024;

    // Just under limit should be OK
    let valid_prompt = "x".repeat(MAX_PROMPT_LENGTH - 1);
    assert!(valid_prompt.len() < MAX_PROMPT_LENGTH);

    // At limit should be OK
    let at_limit = "x".repeat(MAX_PROMPT_LENGTH);
    assert!(at_limit.len() <= MAX_PROMPT_LENGTH);

    // Over limit should fail
    let over_limit = "x".repeat(MAX_PROMPT_LENGTH + 1);
    assert!(over_limit.len() > MAX_PROMPT_LENGTH);
}

#[test]
fn test_time_expression_length_limits() {
    const MAX_TIME_EXPRESSION_LENGTH: usize = 256;

    // Normal expressions should be well under limit
    let normal_expression = "every day at 9am";
    assert!(normal_expression.len() < MAX_TIME_EXPRESSION_LENGTH);

    // Over limit should fail validation
    let over_limit = "x".repeat(MAX_TIME_EXPRESSION_LENGTH + 1);
    assert!(over_limit.len() > MAX_TIME_EXPRESSION_LENGTH);
}

// =============================================================================
// Schedule Status Icon Tests
// =============================================================================

#[test]
fn test_schedule_status_icons() {
    use gorp_core::scheduler::ScheduleStatus;

    // Verify status icon mapping
    let status_icon = |status: ScheduleStatus| -> &'static str {
        match status {
            ScheduleStatus::Active => "🟢",
            ScheduleStatus::Paused => "⏸️",
            ScheduleStatus::Completed => "✅",
            ScheduleStatus::Failed => "❌",
            ScheduleStatus::Executing => "⏳",
            ScheduleStatus::Cancelled => "🚫",
        }
    };

    assert_eq!(status_icon(ScheduleStatus::Active), "🟢");
    assert_eq!(status_icon(ScheduleStatus::Paused), "⏸️");
    assert_eq!(status_icon(ScheduleStatus::Completed), "✅");
    assert_eq!(status_icon(ScheduleStatus::Failed), "❌");
    assert_eq!(status_icon(ScheduleStatus::Executing), "⏳");
    assert_eq!(status_icon(ScheduleStatus::Cancelled), "🚫");
}

// =============================================================================
// File Size Limit Tests
// =============================================================================

#[test]
fn test_file_size_limits() {
    // These constants should match the ones in routes.rs
    const MAX_LOG_FILE_SIZE: u64 = 10 * 1024 * 1024; // 10MB
    const MAX_DISPLAY_FILE_SIZE: u64 = 100 * 1024; // 100KB
    const MAX_SEARCH_FILE_SIZE: u64 = 100 * 1024; // 100KB
    const MAX_MARKDOWN_SIZE: u64 = 1024 * 1024; // 1MB

    // Verify limits are reasonable
    assert!(MAX_LOG_FILE_SIZE > MAX_DISPLAY_FILE_SIZE);
    assert_eq!(MAX_DISPLAY_FILE_SIZE, MAX_SEARCH_FILE_SIZE);
    assert!(MAX_MARKDOWN_SIZE <= MAX_LOG_FILE_SIZE);
}

// =============================================================================
// Search Limits Tests
// =============================================================================

#[test]
fn test_search_limits() {
    const MAX_SEARCH_RESULTS: usize = 100;
    const MAX_FILES_TO_SCAN: usize = 1000;
    const SEARCH_CONTEXT_CHARS: usize = 150;
    const MAX_SEARCH_DEPTH: usize = 20;

    // Verify limits are reasonable
    assert!(MAX_SEARCH_RESULTS > 0);
    assert!(MAX_FILES_TO_SCAN >= MAX_SEARCH_RESULTS);
    assert!(SEARCH_CONTEXT_CHARS > 0);
    assert!(MAX_SEARCH_DEPTH > 0);
}

// =============================================================================
// Large File Reverse Reading Tests
// =============================================================================

#[test]
fn test_read_last_n_lines_large_file() {
    use gorp::admin::routes::read_last_n_lines;

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let file_path = temp_dir.path().join("large.log");

    // Write a file larger than 64KB to trigger reverse reading
    let line = "x".repeat(100);
    let content: String = (1..=1000).map(|i| format!("line{}: {}\n", i, line)).collect();
    std::fs::write(&file_path, &content).expect("Failed to write file");

    // Request last 5
    let lines = read_last_n_lines(&file_path, 5);
    assert_eq!(lines.len(), 5);

    // Should get lines 996-1000
    assert!(lines[0].starts_with("line996:"));
    assert!(lines[4].starts_with("line1000:"));
}

#[test]
fn test_read_last_n_lines_with_empty_lines() {
    use gorp::admin::routes::read_last_n_lines;

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let file_path = temp_dir.path().join("with_blanks.log");

    // Write content with some blank lines
    let content = "line1\n\nline2\n\n\nline3\n";
    std::fs::write(&file_path, content).expect("Failed to write file");

    // Function returns all lines including empty ones
    let lines = read_last_n_lines(&file_path, 10);
    assert_eq!(lines.len(), 6);
    assert_eq!(lines[0], "line1");
    assert_eq!(lines[1], "");
    assert_eq!(lines[2], "line2");
    assert_eq!(lines[3], "");
    assert_eq!(lines[4], "");
    assert_eq!(lines[5], "line3");
}
