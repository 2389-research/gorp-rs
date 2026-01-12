// ABOUTME: Pure helper functions for message handling
// ABOUTME: Channel validation, string truncation, cron detection - all testable without Matrix

use std::path::Path;

/// Check if debug mode is enabled for a channel directory
/// Debug mode is enabled by creating an empty file: .gorp/enable-debug
pub fn is_debug_enabled(channel_dir: &str) -> bool {
    let debug_path = Path::new(channel_dir).join(".gorp").join("enable-debug");
    debug_path.exists()
}

/// Validate a channel name
/// Returns Ok(()) if valid, Err with message if invalid
/// Rules: alphanumeric, dashes, underscores only, max 50 chars, non-empty
pub fn validate_channel_name(name: &str) -> Result<(), &'static str> {
    if name.is_empty() {
        return Err("Channel name cannot be empty");
    }
    if name.len() > 50 {
        return Err("Channel name cannot exceed 50 characters");
    }
    if !name
        .chars()
        .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
    {
        return Err("Channel name can only contain letters, numbers, dashes, and underscores");
    }
    Ok(())
}

/// Truncate a string to max_len characters, adding "..." if truncated
/// Uses character-based slicing to avoid UTF-8 boundary panics
pub fn truncate_str(s: &str, max_len: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= max_len {
        s.to_string()
    } else {
        let truncated: String = chars[..max_len.saturating_sub(3)].iter().collect();
        format!("{}...", truncated)
    }
}

/// Check if a string looks like a cron expression (5 fields: minute hour day month weekday)
/// This is a heuristic, not strict validation - invalid cron expressions will be caught
/// by the cron parser later with a proper error message.
pub fn looks_like_cron(s: &str) -> bool {
    let parts: Vec<&str> = s.split_whitespace().collect();
    // Cron has 5 fields, each containing digits, *, -, /, or ,
    parts.len() == 5
        && parts.iter().all(|p| {
            p.chars()
                .all(|c| c.is_ascii_digit() || c == '*' || c == '-' || c == '/' || c == ',')
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_channel_name_valid() {
        assert!(validate_channel_name("test").is_ok());
        assert!(validate_channel_name("my-channel").is_ok());
        assert!(validate_channel_name("my_channel").is_ok());
    }

    #[test]
    fn test_validate_channel_name_invalid() {
        assert!(validate_channel_name("").is_err());
        assert!(validate_channel_name("a".repeat(51).as_str()).is_err());
        assert!(validate_channel_name("test channel").is_err());
    }

    #[test]
    fn test_truncate_str() {
        assert_eq!(truncate_str("hello", 10), "hello");
        assert_eq!(truncate_str("hello world", 8), "hello...");
    }

    #[test]
    fn test_looks_like_cron() {
        assert!(looks_like_cron("0 9 * * *"));
        assert!(!looks_like_cron("in 5 minutes"));
    }
}
