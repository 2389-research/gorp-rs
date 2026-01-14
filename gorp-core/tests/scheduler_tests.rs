// ABOUTME: Tests for the scheduler module - time parsing and schedule store CRUD
// ABOUTME: Covers natural language parsing, cron expressions, and database operations

use chrono::{Duration, Utc};
use gorp_core::scheduler::{
    compute_next_cron_execution, compute_next_cron_execution_in_tz, parse_time_expression,
    ParsedSchedule, ScheduleStatus, ScheduledPrompt, SchedulerStore,
};
use rusqlite::Connection;
use std::sync::{Arc, Mutex};

/// Helper to create an in-memory scheduler store for testing
fn create_test_store() -> SchedulerStore {
    let conn = Connection::open_in_memory().expect("Failed to create in-memory database");

    // Create the channels table that scheduler depends on (foreign key)
    conn.execute(
        "CREATE TABLE IF NOT EXISTS channels (
            channel_name TEXT PRIMARY KEY,
            room_id TEXT NOT NULL UNIQUE,
            session_id TEXT NOT NULL,
            directory TEXT NOT NULL,
            started INTEGER NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL,
            backend_type TEXT
        )",
        [],
    )
    .expect("Failed to create channels table");

    // Insert test channels that will be referenced by schedules
    let test_channels = vec![
        ("general", "!general:test.com"),
        ("channel-a", "!channel-a:test.com"),
        ("channel-b", "!channel-b:test.com"),
    ];
    for (name, room_id) in test_channels {
        conn.execute(
            "INSERT INTO channels (channel_name, room_id, session_id, directory, created_at)
             VALUES (?1, ?2, 'test-session', '/tmp/test', datetime('now'))",
            rusqlite::params![name, room_id],
        )
        .expect("Failed to insert test channel");
    }

    let store = SchedulerStore::new(Arc::new(Mutex::new(conn)));
    store.initialize_schema().expect("Failed to initialize schema");
    store
}

/// Helper to create a test schedule
fn create_test_schedule(id: &str, channel: &str, prompt: &str) -> ScheduledPrompt {
    ScheduledPrompt {
        id: id.to_string(),
        channel_name: channel.to_string(),
        room_id: format!("!{}:test.com", channel),
        prompt: prompt.to_string(),
        created_by: "@user:test.com".to_string(),
        created_at: Utc::now().to_rfc3339(),
        execute_at: Some((Utc::now() + Duration::hours(1)).to_rfc3339()),
        cron_expression: None,
        last_executed_at: None,
        next_execution_at: (Utc::now() + Duration::hours(1)).to_rfc3339(),
        status: ScheduleStatus::Active,
        error_message: None,
        execution_count: 0,
    }
}

// =============================================================================
// ScheduleStatus Tests
// =============================================================================

#[test]
fn test_schedule_status_display() {
    assert_eq!(ScheduleStatus::Active.to_string(), "active");
    assert_eq!(ScheduleStatus::Paused.to_string(), "paused");
    assert_eq!(ScheduleStatus::Completed.to_string(), "completed");
    assert_eq!(ScheduleStatus::Failed.to_string(), "failed");
    assert_eq!(ScheduleStatus::Executing.to_string(), "executing");
    assert_eq!(ScheduleStatus::Cancelled.to_string(), "cancelled");
}

#[test]
fn test_schedule_status_from_str() {
    assert_eq!(
        "active".parse::<ScheduleStatus>().unwrap(),
        ScheduleStatus::Active
    );
    assert_eq!(
        "paused".parse::<ScheduleStatus>().unwrap(),
        ScheduleStatus::Paused
    );
    assert_eq!(
        "completed".parse::<ScheduleStatus>().unwrap(),
        ScheduleStatus::Completed
    );
    assert_eq!(
        "failed".parse::<ScheduleStatus>().unwrap(),
        ScheduleStatus::Failed
    );
    assert_eq!(
        "executing".parse::<ScheduleStatus>().unwrap(),
        ScheduleStatus::Executing
    );
    assert_eq!(
        "cancelled".parse::<ScheduleStatus>().unwrap(),
        ScheduleStatus::Cancelled
    );
}

#[test]
fn test_schedule_status_from_str_invalid() {
    assert!("invalid".parse::<ScheduleStatus>().is_err());
    assert!("ACTIVE".parse::<ScheduleStatus>().is_err()); // case sensitive
    assert!("".parse::<ScheduleStatus>().is_err());
}

// =============================================================================
// Time Parsing Tests
// =============================================================================

#[test]
fn test_parse_relative_time_minutes() {
    let result = parse_time_expression("in 5 minutes", "UTC").unwrap();
    if let ParsedSchedule::OneTime(dt) = result {
        let diff = dt - Utc::now();
        assert!(diff.num_minutes() >= 4 && diff.num_minutes() <= 6);
    } else {
        panic!("Expected OneTime schedule");
    }
}

#[test]
fn test_parse_relative_time_hours() {
    let result = parse_time_expression("in 2 hours", "UTC").unwrap();
    if let ParsedSchedule::OneTime(dt) = result {
        let diff = dt - Utc::now();
        assert!(diff.num_hours() >= 1 && diff.num_hours() <= 3);
    } else {
        panic!("Expected OneTime schedule");
    }
}

#[test]
fn test_parse_relative_time_days() {
    let result = parse_time_expression("in 3 days", "UTC").unwrap();
    if let ParsedSchedule::OneTime(dt) = result {
        let diff = dt - Utc::now();
        assert!(diff.num_days() >= 2 && diff.num_days() <= 4);
    } else {
        panic!("Expected OneTime schedule");
    }
}

#[test]
fn test_parse_relative_time_variants() {
    // Test various minute aliases
    assert!(parse_time_expression("in 1 min", "UTC").is_ok());
    assert!(parse_time_expression("in 10 mins", "UTC").is_ok());
    assert!(parse_time_expression("in 1 minute", "UTC").is_ok());

    // Test hour aliases
    assert!(parse_time_expression("in 1 hr", "UTC").is_ok());
    assert!(parse_time_expression("in 2 hrs", "UTC").is_ok());
    assert!(parse_time_expression("in 1 hour", "UTC").is_ok());

    // Test day aliases
    assert!(parse_time_expression("in 1 day", "UTC").is_ok());
}

#[test]
fn test_parse_recurring_hourly() {
    let result = parse_time_expression("every hour", "UTC").unwrap();
    if let ParsedSchedule::Recurring { cron, next: _ } = result {
        assert_eq!(cron, "0 * * * *");
    } else {
        panic!("Expected Recurring schedule");
    }
}

#[test]
fn test_parse_recurring_daily() {
    let result = parse_time_expression("every day", "UTC").unwrap();
    if let ParsedSchedule::Recurring { cron, next: _ } = result {
        assert_eq!(cron, "0 9 * * *"); // Default 9am
    } else {
        panic!("Expected Recurring schedule");
    }
}

#[test]
fn test_parse_recurring_daily_at_time() {
    let result = parse_time_expression("every day at 8am", "UTC").unwrap();
    if let ParsedSchedule::Recurring { cron, next: _ } = result {
        assert_eq!(cron, "0 8 * * *");
    } else {
        panic!("Expected Recurring schedule");
    }
}

#[test]
fn test_parse_recurring_weekday() {
    let result = parse_time_expression("every monday 9am", "UTC").unwrap();
    if let ParsedSchedule::Recurring { cron, next: _ } = result {
        assert_eq!(cron, "0 9 * * MON");
    } else {
        panic!("Expected Recurring schedule");
    }
}

#[test]
fn test_parse_recurring_weekday_variants() {
    // Test various weekday spellings
    let cases = vec![
        ("every mon 8am", "0 8 * * MON"),
        ("every tue 9am", "0 9 * * TUE"),
        ("every wed 10am", "0 10 * * WED"),
        ("every thu 11am", "0 11 * * THU"),
        ("every fri 2pm", "0 14 * * FRI"),
        ("every sat 3pm", "0 15 * * SAT"),
        ("every sun 4pm", "0 16 * * SUN"),
    ];

    for (input, expected_cron) in cases {
        let result = parse_time_expression(input, "UTC").unwrap();
        if let ParsedSchedule::Recurring { cron, next: _ } = result {
            assert_eq!(cron, expected_cron, "Failed for input: {}", input);
        } else {
            panic!("Expected Recurring schedule for: {}", input);
        }
    }
}

#[test]
fn test_parse_recurring_every_n_minutes() {
    let result = parse_time_expression("every 15 minutes", "UTC").unwrap();
    if let ParsedSchedule::Recurring { cron, next: _ } = result {
        assert_eq!(cron, "*/15 * * * *");
    } else {
        panic!("Expected Recurring schedule");
    }
}

#[test]
fn test_parse_recurring_every_n_hours() {
    let result = parse_time_expression("every 2 hours", "UTC").unwrap();
    if let ParsedSchedule::Recurring { cron, next: _ } = result {
        assert_eq!(cron, "0 */2 * * *");
    } else {
        panic!("Expected Recurring schedule");
    }
}

#[test]
fn test_parse_recurring_morning_afternoon_evening() {
    let cases = vec![
        ("every morning", "0 8 * * *"),
        ("every afternoon", "0 14 * * *"),
        ("every evening", "0 18 * * *"),
        ("every night", "0 21 * * *"),
    ];

    for (input, expected_cron) in cases {
        let result = parse_time_expression(input, "UTC").unwrap();
        if let ParsedSchedule::Recurring { cron, next: _ } = result {
            assert_eq!(cron, expected_cron, "Failed for input: {}", input);
        } else {
            panic!("Expected Recurring schedule for: {}", input);
        }
    }
}

#[test]
fn test_parse_everyday_alias() {
    // "everyday" should be normalized to "every day"
    let result = parse_time_expression("everyday 9am", "UTC").unwrap();
    if let ParsedSchedule::Recurring { cron, next: _ } = result {
        assert_eq!(cron, "0 9 * * *");
    } else {
        panic!("Expected Recurring schedule");
    }
}

#[test]
fn test_parse_invalid_expressions() {
    assert!(parse_time_expression("gobbledygook", "UTC").is_err());
    assert!(parse_time_expression("every 0 minutes", "UTC").is_err());
    assert!(parse_time_expression("every 60 minutes", "UTC").is_err());
    assert!(parse_time_expression("every 0 hours", "UTC").is_err());
    assert!(parse_time_expression("every 24 hours", "UTC").is_err());
}

// =============================================================================
// Cron Execution Tests
// =============================================================================

#[test]
fn test_compute_next_cron_execution_utc() {
    let next = compute_next_cron_execution("0 9 * * *").unwrap();
    assert!(next > Utc::now());
}

#[test]
fn test_compute_next_cron_execution_in_tz() {
    let next = compute_next_cron_execution_in_tz("0 9 * * *", "America/New_York").unwrap();
    assert!(next > Utc::now());
}

#[test]
fn test_compute_next_cron_execution_invalid_cron() {
    assert!(compute_next_cron_execution("invalid cron").is_err());
}

#[test]
fn test_compute_next_cron_execution_invalid_timezone() {
    assert!(compute_next_cron_execution_in_tz("0 9 * * *", "Invalid/Timezone").is_err());
}

// =============================================================================
// SchedulerStore CRUD Tests
// =============================================================================

#[test]
fn test_store_create_and_get_schedule() {
    let store = create_test_store();
    let schedule = create_test_schedule("test-id-1", "general", "Test prompt");

    store.create_schedule(&schedule).unwrap();

    let retrieved = store.get_by_id("test-id-1").unwrap();
    assert!(retrieved.is_some());

    let retrieved = retrieved.unwrap();
    assert_eq!(retrieved.id, "test-id-1");
    assert_eq!(retrieved.channel_name, "general");
    assert_eq!(retrieved.prompt, "Test prompt");
    assert_eq!(retrieved.status, ScheduleStatus::Active);
}

#[test]
fn test_store_get_nonexistent() {
    let store = create_test_store();
    let result = store.get_by_id("nonexistent").unwrap();
    assert!(result.is_none());
}

#[test]
fn test_store_list_all() {
    let store = create_test_store();

    store
        .create_schedule(&create_test_schedule("id-1", "channel-a", "Prompt 1"))
        .unwrap();
    store
        .create_schedule(&create_test_schedule("id-2", "channel-b", "Prompt 2"))
        .unwrap();
    store
        .create_schedule(&create_test_schedule("id-3", "channel-a", "Prompt 3"))
        .unwrap();

    let all = store.list_all().unwrap();
    assert_eq!(all.len(), 3);
}

#[test]
fn test_store_list_by_room() {
    let store = create_test_store();

    store
        .create_schedule(&create_test_schedule("id-1", "channel-a", "Prompt 1"))
        .unwrap();
    store
        .create_schedule(&create_test_schedule("id-2", "channel-b", "Prompt 2"))
        .unwrap();
    store
        .create_schedule(&create_test_schedule("id-3", "channel-a", "Prompt 3"))
        .unwrap();

    let by_room = store.list_by_room("!channel-a:test.com").unwrap();
    assert_eq!(by_room.len(), 2);
}

#[test]
fn test_store_list_by_channel() {
    let store = create_test_store();

    store
        .create_schedule(&create_test_schedule("id-1", "channel-a", "Prompt 1"))
        .unwrap();
    store
        .create_schedule(&create_test_schedule("id-2", "channel-b", "Prompt 2"))
        .unwrap();
    store
        .create_schedule(&create_test_schedule("id-3", "channel-a", "Prompt 3"))
        .unwrap();

    let by_channel = store.list_by_channel("channel-a").unwrap();
    assert_eq!(by_channel.len(), 2);
}

#[test]
fn test_store_delete_schedule() {
    let store = create_test_store();
    let schedule = create_test_schedule("delete-me", "general", "To be deleted");

    store.create_schedule(&schedule).unwrap();
    assert!(store.get_by_id("delete-me").unwrap().is_some());

    let deleted = store.delete_schedule("delete-me").unwrap();
    assert!(deleted);

    assert!(store.get_by_id("delete-me").unwrap().is_none());
}

#[test]
fn test_store_delete_nonexistent() {
    let store = create_test_store();
    let deleted = store.delete_schedule("nonexistent").unwrap();
    assert!(!deleted);
}

#[test]
fn test_store_pause_and_resume() {
    let store = create_test_store();
    let schedule = create_test_schedule("pause-test", "general", "Pause test");

    store.create_schedule(&schedule).unwrap();

    // Pause
    let paused = store.pause_schedule("pause-test").unwrap();
    assert!(paused);

    let retrieved = store.get_by_id("pause-test").unwrap().unwrap();
    assert_eq!(retrieved.status, ScheduleStatus::Paused);

    // Resume
    let resumed = store.resume_schedule("pause-test").unwrap();
    assert!(resumed);

    let retrieved = store.get_by_id("pause-test").unwrap().unwrap();
    assert_eq!(retrieved.status, ScheduleStatus::Active);
}

#[test]
fn test_store_pause_already_paused() {
    let store = create_test_store();
    let schedule = create_test_schedule("pause-test", "general", "Pause test");

    store.create_schedule(&schedule).unwrap();
    store.pause_schedule("pause-test").unwrap();

    // Pause again - should return false
    let paused_again = store.pause_schedule("pause-test").unwrap();
    assert!(!paused_again);
}

#[test]
fn test_store_resume_not_paused() {
    let store = create_test_store();
    let schedule = create_test_schedule("resume-test", "general", "Resume test");

    store.create_schedule(&schedule).unwrap();

    // Resume without pausing first - should return false
    let resumed = store.resume_schedule("resume-test").unwrap();
    assert!(!resumed);
}

#[test]
fn test_store_cancel_schedule() {
    let store = create_test_store();
    let schedule = create_test_schedule("cancel-test", "general", "Cancel test");

    store.create_schedule(&schedule).unwrap();

    let cancelled = store.cancel_schedule("cancel-test").unwrap();
    assert!(cancelled);

    let retrieved = store.get_by_id("cancel-test").unwrap().unwrap();
    assert_eq!(retrieved.status, ScheduleStatus::Cancelled);
}

#[test]
fn test_store_mark_failed() {
    let store = create_test_store();
    let schedule = create_test_schedule("fail-test", "general", "Fail test");

    store.create_schedule(&schedule).unwrap();
    store.mark_failed("fail-test", "Test error message").unwrap();

    let retrieved = store.get_by_id("fail-test").unwrap().unwrap();
    assert_eq!(retrieved.status, ScheduleStatus::Failed);
    assert_eq!(retrieved.error_message, Some("Test error message".to_string()));
}

#[test]
fn test_store_mark_executed_recurring() {
    let store = create_test_store();
    let mut schedule = create_test_schedule("recurring-test", "general", "Recurring");
    schedule.cron_expression = Some("0 9 * * *".to_string());
    schedule.execute_at = None;

    store.create_schedule(&schedule).unwrap();

    let next_execution = Utc::now() + Duration::days(1);
    store
        .mark_executed("recurring-test", Some(next_execution))
        .unwrap();

    let retrieved = store.get_by_id("recurring-test").unwrap().unwrap();
    assert_eq!(retrieved.status, ScheduleStatus::Active);
    assert_eq!(retrieved.execution_count, 1);
    assert!(retrieved.last_executed_at.is_some());
}

#[test]
fn test_store_mark_executed_onetime() {
    let store = create_test_store();
    let schedule = create_test_schedule("onetime-test", "general", "One-time");

    store.create_schedule(&schedule).unwrap();

    // Mark as executed with no next execution (one-time)
    store.mark_executed("onetime-test", None).unwrap();

    let retrieved = store.get_by_id("onetime-test").unwrap().unwrap();
    assert_eq!(retrieved.status, ScheduleStatus::Completed);
    assert_eq!(retrieved.execution_count, 1);
}

#[test]
fn test_store_claim_due_schedules() {
    let store = create_test_store();

    // Create a schedule that's due now
    let mut due_schedule = create_test_schedule("due-now", "general", "Due now");
    due_schedule.next_execution_at = (Utc::now() - Duration::minutes(1)).to_rfc3339();
    store.create_schedule(&due_schedule).unwrap();

    // Create a schedule that's not due yet
    let future_schedule = create_test_schedule("future", "general", "Future");
    store.create_schedule(&future_schedule).unwrap();

    // Claim due schedules
    let claimed = store.claim_due_schedules(Utc::now()).unwrap();
    assert_eq!(claimed.len(), 1);
    assert_eq!(claimed[0].id, "due-now");

    // Verify the claimed schedule is now in executing state
    let retrieved = store.get_by_id("due-now").unwrap().unwrap();
    assert_eq!(retrieved.status, ScheduleStatus::Executing);
}

#[test]
fn test_store_claim_due_schedules_none_due() {
    let store = create_test_store();

    let future_schedule = create_test_schedule("future", "general", "Future");
    store.create_schedule(&future_schedule).unwrap();

    let claimed = store.claim_due_schedules(Utc::now()).unwrap();
    assert!(claimed.is_empty());
}

#[test]
fn test_store_get_schedule_alias() {
    let store = create_test_store();
    let schedule = create_test_schedule("alias-test", "general", "Alias test");

    store.create_schedule(&schedule).unwrap();

    // get_schedule is an alias for get_by_id
    let retrieved = store.get_schedule("alias-test").unwrap();
    assert!(retrieved.is_some());
    assert_eq!(retrieved.unwrap().id, "alias-test");
}
