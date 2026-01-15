// ABOUTME: Schedule import and parsing utilities
// ABOUTME: Handles YAML schedule imports and natural language time parsing

use super::helpers::looks_like_cron;
use crate::scheduler::{
    compute_next_cron_execution_in_tz, parse_time_expression, ParsedSchedule, ScheduleStatus,
    ScheduledPrompt, SchedulerStore,
};
use crate::session::Channel;

/// Import a single schedule from YAML data
pub fn import_schedule(
    time: &str,
    prompt: &str,
    paused: bool,
    channel: &Channel,
    sender: &str,
    timezone: &str,
    scheduler_store: &SchedulerStore,
) -> anyhow::Result<()> {
    // Check if time is a raw cron expression (exported from recurring schedule)
    let parsed = if looks_like_cron(time) {
        // Parse as raw cron expression
        let next = compute_next_cron_execution_in_tz(time, timezone)?;
        ParsedSchedule::Recurring {
            cron: time.to_string(),
            next,
        }
    } else {
        // Try parsing as natural language time expression
        parse_time_expression(time, timezone)?
    };

    let schedule_id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();

    let (execute_at, cron_expr, next_exec) = match &parsed {
        ParsedSchedule::OneTime(dt) => (Some(dt.to_rfc3339()), None, dt.to_rfc3339()),
        ParsedSchedule::Recurring { cron, next } => (None, Some(cron.clone()), next.to_rfc3339()),
    };

    let status = if paused {
        ScheduleStatus::Paused
    } else {
        ScheduleStatus::Active
    };

    let scheduled_prompt = ScheduledPrompt {
        id: schedule_id,
        channel_name: channel.channel_name.clone(),
        room_id: channel.room_id.clone(),
        prompt: prompt.to_string(),
        created_by: sender.to_string(),
        created_at: now,
        execute_at,
        cron_expression: cron_expr,
        last_executed_at: None,
        next_execution_at: next_exec,
        status,
        error_message: None,
        execution_count: 0,
    };

    scheduler_store.create_schedule(&scheduled_prompt)?;
    Ok(())
}

/// Parse schedule input to extract time expression and prompt
/// Uses greedy matching with a max lookahead to avoid consuming the entire prompt
pub fn parse_schedule_input(
    input: &str,
    timezone: &str,
) -> anyhow::Result<(ParsedSchedule, String)> {
    let words: Vec<&str> = input.split_whitespace().collect();

    // Require at least 1 word for prompt, limit time expression to 10 words max
    let max_time_words = std::cmp::min(words.len().saturating_sub(1), 10);

    // Try progressively longer prefixes until parsing fails
    let mut last_valid: Option<(ParsedSchedule, usize)> = None;

    for end_idx in 1..=max_time_words {
        let time_expr = words[..end_idx].join(" ");
        if let Ok(schedule) = parse_time_expression(&time_expr, timezone) {
            last_valid = Some((schedule, end_idx));
        }
    }

    match last_valid {
        Some((schedule, word_count)) => {
            let prompt = words[word_count..].join(" ");
            Ok((schedule, prompt))
        }
        None => anyhow::bail!(
            "Could not parse time expression. Try: 'in 2 hours', 'tomorrow 9am', 'every monday 8am'"
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;
    use std::sync::{Arc, Mutex};

    /// Create a test store with an in-memory database
    fn create_test_store(channel_name: &str, room_id: &str) -> SchedulerStore {
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

        // Insert the test channel
        conn.execute(
            "INSERT INTO channels (channel_name, room_id, session_id, directory, created_at)
             VALUES (?1, ?2, 'test-session', '/tmp/test', datetime('now'))",
            rusqlite::params![channel_name, room_id],
        )
        .expect("Failed to insert test channel");

        let store = SchedulerStore::new(Arc::new(Mutex::new(conn)));
        store
            .initialize_schema()
            .expect("Failed to initialize schema");
        store
    }

    fn make_test_channel() -> Channel {
        Channel {
            channel_name: "test-channel".to_string(),
            room_id: "!testroom:example.org".to_string(),
            session_id: "test-session".to_string(),
            directory: "/tmp/test".to_string(),
            started: false,
            created_at: chrono::Utc::now().to_rfc3339(),
            backend_type: None,
            is_dispatch_room: false,
        }
    }

    #[test]
    fn test_parse_schedule_input_relative() {
        let result = parse_schedule_input("in 5 minutes do something", "UTC");
        assert!(result.is_ok());
        let (schedule, prompt) = result.unwrap();
        assert!(matches!(schedule, ParsedSchedule::OneTime(_)));
        assert_eq!(prompt, "do something");
    }

    #[test]
    fn test_parse_schedule_input_recurring() {
        let result = parse_schedule_input("every day at 9am check server", "UTC");
        assert!(result.is_ok());
        let (schedule, prompt) = result.unwrap();
        assert!(matches!(schedule, ParsedSchedule::Recurring { .. }));
        assert_eq!(prompt, "check server");
    }

    #[test]
    fn test_parse_schedule_input_invalid() {
        let result = parse_schedule_input("banana apple cherry", "UTC");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_schedule_input_single_word() {
        // Need at least time + prompt, single word can't parse
        let result = parse_schedule_input("tomorrow", "UTC");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_schedule_input_long_prompt() {
        let result = parse_schedule_input(
            "in 1 hour check the status of all running services and report back",
            "UTC",
        );
        assert!(result.is_ok());
        let (schedule, prompt) = result.unwrap();
        assert!(matches!(schedule, ParsedSchedule::OneTime(_)));
        assert_eq!(
            prompt,
            "check the status of all running services and report back"
        );
    }

    #[test]
    fn test_import_schedule_onetime() {
        let channel = make_test_channel();
        let store = create_test_store(&channel.channel_name, &channel.room_id);

        let result = import_schedule(
            "in 30 minutes",
            "test prompt",
            false,
            &channel,
            "@user:example.org",
            "UTC",
            &store,
        );
        assert!(result.is_ok());

        // Verify the schedule was created
        let schedules = store.list_by_channel(&channel.channel_name).unwrap();
        assert_eq!(schedules.len(), 1);
        assert_eq!(schedules[0].prompt, "test prompt");
        assert_eq!(schedules[0].channel_name, "test-channel");
        assert_eq!(schedules[0].status, ScheduleStatus::Active);
        assert!(schedules[0].execute_at.is_some());
        assert!(schedules[0].cron_expression.is_none());
    }

    #[test]
    fn test_import_schedule_recurring() {
        let channel = make_test_channel();
        let store = create_test_store(&channel.channel_name, &channel.room_id);

        let result = import_schedule(
            "every monday 9am", // Note: format is "every <day> <time>" without "at"
            "weekly status check",
            false,
            &channel,
            "@user:example.org",
            "UTC",
            &store,
        );
        assert!(result.is_ok());

        let schedules = store.list_by_channel(&channel.channel_name).unwrap();
        assert_eq!(schedules.len(), 1);
        assert_eq!(schedules[0].prompt, "weekly status check");
        assert!(schedules[0].execute_at.is_none());
        assert!(schedules[0].cron_expression.is_some());
    }

    #[test]
    fn test_import_schedule_paused() {
        let channel = make_test_channel();
        let store = create_test_store(&channel.channel_name, &channel.room_id);

        let result = import_schedule(
            "in 1 hour",
            "paused task",
            true, // paused
            &channel,
            "@user:example.org",
            "UTC",
            &store,
        );
        assert!(result.is_ok());

        let schedules = store.list_by_channel(&channel.channel_name).unwrap();
        assert_eq!(schedules.len(), 1);
        assert_eq!(schedules[0].status, ScheduleStatus::Paused);
    }

    #[test]
    fn test_import_schedule_raw_cron() {
        let channel = make_test_channel();
        let store = create_test_store(&channel.channel_name, &channel.room_id);

        // Import a raw cron expression (as exported schedules might have)
        let result = import_schedule(
            "0 9 * * 1", // Every Monday at 9am
            "cron scheduled task",
            false,
            &channel,
            "@user:example.org",
            "UTC",
            &store,
        );
        assert!(result.is_ok());

        let schedules = store.list_by_channel(&channel.channel_name).unwrap();
        assert_eq!(schedules.len(), 1);
        assert!(schedules[0].cron_expression.is_some());
        assert_eq!(schedules[0].cron_expression.as_ref().unwrap(), "0 9 * * 1");
    }

    #[test]
    fn test_import_schedule_invalid_time() {
        let channel = make_test_channel();
        let store = create_test_store(&channel.channel_name, &channel.room_id);

        let result = import_schedule(
            "not a valid time",
            "test prompt",
            false,
            &channel,
            "@user:example.org",
            "UTC",
            &store,
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_import_schedule_preserves_sender() {
        let channel = make_test_channel();
        let store = create_test_store(&channel.channel_name, &channel.room_id);

        let result = import_schedule(
            "in 2 hours",
            "sender test",
            false,
            &channel,
            "@specificuser:matrix.org",
            "UTC",
            &store,
        );
        assert!(result.is_ok());

        let schedules = store.list_by_channel(&channel.channel_name).unwrap();
        assert_eq!(schedules[0].created_by, "@specificuser:matrix.org");
    }

    #[test]
    fn test_import_multiple_schedules() {
        let channel = make_test_channel();
        let store = create_test_store(&channel.channel_name, &channel.room_id);

        // Import multiple schedules
        import_schedule(
            "in 1 hour",
            "task 1",
            false,
            &channel,
            "@user:example.org",
            "UTC",
            &store,
        )
        .unwrap();

        import_schedule(
            "in 2 hours",
            "task 2",
            false,
            &channel,
            "@user:example.org",
            "UTC",
            &store,
        )
        .unwrap();

        import_schedule(
            "every day at 12pm", // Use 12pm instead of "noon"
            "task 3",
            true,
            &channel,
            "@user:example.org",
            "UTC",
            &store,
        )
        .unwrap();

        let schedules = store.list_by_channel(&channel.channel_name).unwrap();
        assert_eq!(schedules.len(), 3);
    }
}
