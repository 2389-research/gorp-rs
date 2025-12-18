// ABOUTME: Scheduler system for executing Claude prompts at specified times.
// ABOUTME: Supports one-time and recurring (cron) schedules with natural language parsing.

use anyhow::{Context, Result};
use chrono::{DateTime, TimeZone, Utc};
use cron::Schedule;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use std::sync::{Arc, Mutex};

use crate::metrics;

/// Represents a scheduled prompt in the database
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduledPrompt {
    pub id: String,
    pub channel_name: String,
    pub room_id: String,
    pub prompt: String,
    pub created_by: String,
    pub created_at: String,
    pub execute_at: Option<String>,
    pub cron_expression: Option<String>,
    pub last_executed_at: Option<String>,
    pub next_execution_at: String,
    pub status: ScheduleStatus,
    pub error_message: Option<String>,
    pub execution_count: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ScheduleStatus {
    Active,
    Paused,
    Completed,
    Failed,
    Executing,
    Cancelled,
}

impl std::fmt::Display for ScheduleStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ScheduleStatus::Active => write!(f, "active"),
            ScheduleStatus::Paused => write!(f, "paused"),
            ScheduleStatus::Completed => write!(f, "completed"),
            ScheduleStatus::Failed => write!(f, "failed"),
            ScheduleStatus::Executing => write!(f, "executing"),
            ScheduleStatus::Cancelled => write!(f, "cancelled"),
        }
    }
}

impl FromStr for ScheduleStatus {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self> {
        match s {
            "active" => Ok(ScheduleStatus::Active),
            "paused" => Ok(ScheduleStatus::Paused),
            "completed" => Ok(ScheduleStatus::Completed),
            "failed" => Ok(ScheduleStatus::Failed),
            "executing" => Ok(ScheduleStatus::Executing),
            "cancelled" => Ok(ScheduleStatus::Cancelled),
            _ => anyhow::bail!("Unknown schedule status: {}", s),
        }
    }
}

/// Result of parsing a time expression
#[derive(Debug)]
pub enum ParsedSchedule {
    OneTime(DateTime<Utc>),
    Recurring { cron: String, next: DateTime<Utc> },
}

/// Parse relative time expressions like "in 5 minutes", "in 2 hours"
fn parse_relative_time(input: &str) -> Option<Result<ParsedSchedule>> {
    // Match patterns like "in X minutes", "in X hours", "in X days"
    let re =
        regex::Regex::new(r"^in\s+(\d+)\s+(minute|minutes|min|mins|hour|hours|hr|hrs|day|days)$")
            .ok()?;

    let caps = re.captures(input)?;
    let amount: i64 = caps.get(1)?.as_str().parse().ok()?;
    let unit = caps.get(2)?.as_str();

    let duration = match unit {
        "minute" | "minutes" | "min" | "mins" => chrono::Duration::minutes(amount),
        "hour" | "hours" | "hr" | "hrs" => chrono::Duration::hours(amount),
        "day" | "days" => chrono::Duration::days(amount),
        _ => return None,
    };

    let target_time = Utc::now() + duration;
    Some(Ok(ParsedSchedule::OneTime(target_time)))
}

/// Parse natural language time expression into a schedule
pub fn parse_time_expression(input: &str, timezone: &str) -> Result<ParsedSchedule> {
    let input_lower = input.to_lowercase().trim().to_string();
    // Normalize common aliases
    let input_lower = input_lower.replace("everyday", "every day");

    tracing::debug!(input = %input_lower, timezone = %timezone, "Attempting to parse time expression");

    // Check for recurring patterns first
    if input_lower.starts_with("every ") {
        return parse_recurring(&input_lower, timezone);
    }

    // Handle "in X minutes/hours/days" patterns manually
    if let Some(result) = parse_relative_time(&input_lower) {
        return result;
    }

    // Try two_timer for natural language one-time expressions
    let now = chrono::Local::now();
    let config = two_timer::Config::new().now(now.naive_local());

    match two_timer::parse(&input_lower, Some(config)) {
        Ok((start, _end, _)) => {
            tracing::debug!(start = ?start, "two_timer parsed successfully");
            let dt = chrono::Local.from_local_datetime(&start).single();
            match dt {
                Some(local_dt) => {
                    let utc_dt = local_dt.with_timezone(&Utc);
                    if utc_dt <= Utc::now() {
                        tracing::debug!(utc_dt = %utc_dt, "Parsed time is in the past");
                        anyhow::bail!("Scheduled time must be in the future");
                    }
                    tracing::debug!(utc_dt = %utc_dt, "Successfully parsed time expression");
                    Ok(ParsedSchedule::OneTime(utc_dt))
                }
                None => anyhow::bail!("Ambiguous time expression"),
            }
        }
        Err(e) => {
            tracing::debug!(error = ?e, input = %input_lower, "two_timer failed to parse");
            anyhow::bail!(
                "Could not parse time expression '{}'. Try: 'in 2 hours', 'tomorrow 9am', 'every monday 8am'",
                input
            )
        }
    }
}

/// Parse recurring schedule patterns like "every monday 8am"
fn parse_recurring(input: &str, timezone: &str) -> Result<ParsedSchedule> {
    let rest = input.strip_prefix("every ").unwrap_or(input);

    // Parse common recurring patterns
    let cron = if rest == "hour" || rest == "hourly" {
        "0 * * * *".to_string()
    } else if rest == "day" || rest == "daily" {
        "0 9 * * *".to_string() // Default to 9am
    } else if rest.starts_with("day at ") || rest.starts_with("day ") {
        let time_part = rest
            .strip_prefix("day at ")
            .or_else(|| rest.strip_prefix("day "))
            .unwrap_or("9am");
        let (hour, minute) = parse_time_of_day(time_part)?;
        format!("{} {} * * *", minute, hour)
    } else if let Some(time_part) = rest.strip_prefix("morning ") {
        // "every morning 7:30am" -> daily at specified time
        let (hour, minute) = parse_time_of_day(time_part.trim())?;
        format!("{} {} * * *", minute, hour)
    } else if rest == "morning" {
        // "every morning" -> daily at 8am
        "0 8 * * *".to_string()
    } else if let Some(time_part) = rest.strip_prefix("afternoon ") {
        // "every afternoon 2pm" -> daily at specified time
        let (hour, minute) = parse_time_of_day(time_part.trim())?;
        format!("{} {} * * *", minute, hour)
    } else if rest == "afternoon" {
        // "every afternoon" -> daily at 2pm
        "0 14 * * *".to_string()
    } else if let Some(time_part) = rest.strip_prefix("evening ") {
        // "every evening 6pm" -> daily at specified time
        let (hour, minute) = parse_time_of_day(time_part.trim())?;
        format!("{} {} * * *", minute, hour)
    } else if rest == "evening" {
        // "every evening" -> daily at 6pm
        "0 18 * * *".to_string()
    } else if let Some(time_part) = rest.strip_prefix("night ") {
        // "every night 9pm" -> daily at specified time
        let (hour, minute) = parse_time_of_day(time_part.trim())?;
        format!("{} {} * * *", minute, hour)
    } else if rest == "night" {
        // "every night" -> daily at 9pm
        "0 21 * * *".to_string()
    } else if let Some(minutes) = rest.strip_suffix(" minutes") {
        let mins: u32 = minutes.trim().parse().context("Invalid minute value")?;
        if mins == 0 || mins > 59 {
            anyhow::bail!("Minutes must be between 1 and 59");
        }
        format!("*/{} * * * *", mins)
    } else if let Some(hours) = rest.strip_suffix(" hours") {
        let hrs: u32 = hours.trim().parse().context("Invalid hour value")?;
        if hrs == 0 || hrs > 23 {
            anyhow::bail!("Hours must be between 1 and 23");
        }
        format!("0 */{} * * *", hrs)
    } else {
        // Try to parse as "weekday time" like "monday 8am"
        parse_weekday_time(rest)?
    };

    // Validate the cron expression and compute next execution in the configured timezone
    let next = compute_next_cron_execution_in_tz(&cron, timezone)?;

    Ok(ParsedSchedule::Recurring { cron, next })
}

/// Parse time of day like "8am", "14:30", "2pm"
fn parse_time_of_day(input: &str) -> Result<(u32, u32)> {
    let input = input.trim().to_lowercase();

    // Try parsing "8am", "8 am", "8:00am"
    if input.ends_with("am") || input.ends_with("pm") {
        let is_pm = input.ends_with("pm");
        let time_str = input
            .strip_suffix("am")
            .or_else(|| input.strip_suffix("pm"))
            .unwrap()
            .trim();

        let (hour, minute) = if time_str.contains(':') {
            let parts: Vec<&str> = time_str.split(':').collect();
            let h: u32 = parts[0].parse().context("Invalid hour")?;
            let m: u32 = parts
                .get(1)
                .unwrap_or(&"0")
                .parse()
                .context("Invalid minute")?;
            (h, m)
        } else {
            let h: u32 = time_str.parse().context("Invalid hour")?;
            (h, 0)
        };

        let hour = if is_pm && hour < 12 {
            hour + 12
        } else if !is_pm && hour == 12 {
            0
        } else {
            hour
        };

        if hour > 23 || minute > 59 {
            anyhow::bail!("Invalid time: hour must be 0-23, minute must be 0-59");
        }

        Ok((hour, minute))
    } else if input.contains(':') {
        // Try 24-hour format "14:30"
        let parts: Vec<&str> = input.split(':').collect();
        let hour: u32 = parts[0].parse().context("Invalid hour")?;
        let minute: u32 = parts
            .get(1)
            .unwrap_or(&"0")
            .parse()
            .context("Invalid minute")?;

        if hour > 23 || minute > 59 {
            anyhow::bail!("Invalid time: hour must be 0-23, minute must be 0-59");
        }

        Ok((hour, minute))
    } else {
        // Just a number - assume it's the hour
        let hour: u32 = input.parse().context("Invalid hour")?;
        if hour > 23 {
            anyhow::bail!("Hour must be 0-23");
        }
        Ok((hour, 0))
    }
}

/// Parse weekday patterns like "monday 8am", "fri 2pm"
fn parse_weekday_time(input: &str) -> Result<String> {
    let parts: Vec<&str> = input.split_whitespace().collect();
    if parts.is_empty() {
        anyhow::bail!("Empty recurring pattern");
    }

    let weekday = parts[0];
    let cron_day = match weekday {
        "monday" | "mon" => "MON",
        "tuesday" | "tue" | "tues" => "TUE",
        "wednesday" | "wed" => "WED",
        "thursday" | "thu" | "thur" | "thurs" => "THU",
        "friday" | "fri" => "FRI",
        "saturday" | "sat" => "SAT",
        "sunday" | "sun" => "SUN",
        "weekday" | "weekdays" => "MON-FRI",
        "weekend" | "weekends" => "SAT,SUN",
        _ => anyhow::bail!(
            "Unknown day '{}'. Use: monday, tuesday, wednesday, thursday, friday, saturday, sunday",
            weekday
        ),
    };

    let (hour, minute) = if parts.len() > 1 {
        // Join remaining parts for time parsing (handles "8 am" as two parts)
        let time_str = parts[1..].join("");
        parse_time_of_day(&time_str)?
    } else {
        (9, 0) // Default to 9am
    };

    Ok(format!("{} {} * * {}", minute, hour, cron_day))
}

/// Compute the next execution time for a cron expression in the given timezone
pub fn compute_next_cron_execution(cron_expr: &str) -> Result<DateTime<Utc>> {
    compute_next_cron_execution_in_tz(cron_expr, "UTC")
}

/// Compute the next execution time for a cron expression in the given timezone
pub fn compute_next_cron_execution_in_tz(cron_expr: &str, timezone: &str) -> Result<DateTime<Utc>> {
    // Cron crate expects 6-field expressions (with seconds), but we use 5-field
    // Prepend "0 " for seconds
    let cron_with_seconds = format!("0 {}", cron_expr);

    let schedule = Schedule::from_str(&cron_with_seconds)
        .with_context(|| format!("Invalid cron expression: {}", cron_expr))?;

    // Parse timezone and compute next execution in that timezone
    let tz: chrono_tz::Tz = timezone
        .parse()
        .with_context(|| format!("Invalid timezone: {}", timezone))?;

    let next_local = schedule
        .upcoming(tz)
        .next()
        .context("Could not compute next execution time")?;

    // Convert to UTC for storage
    Ok(next_local.with_timezone(&Utc))
}

/// Scheduler store for database operations
#[derive(Clone)]
pub struct SchedulerStore {
    db: Arc<Mutex<Connection>>,
}

impl SchedulerStore {
    pub fn new(db: Arc<Mutex<Connection>>) -> Self {
        Self { db }
    }

    /// Initialize the database schema
    pub fn initialize_schema(&self) -> Result<()> {
        let conn = self
            .db
            .lock()
            .map_err(|e| anyhow::anyhow!("Database mutex poisoned: {}", e))?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS scheduled_prompts (
                id TEXT PRIMARY KEY,
                channel_name TEXT NOT NULL,
                room_id TEXT NOT NULL,
                prompt TEXT NOT NULL,
                created_by TEXT NOT NULL,
                created_at TEXT NOT NULL,
                execute_at TEXT,
                cron_expression TEXT,
                last_executed_at TEXT,
                next_execution_at TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'active',
                error_message TEXT,
                execution_count INTEGER DEFAULT 0,
                FOREIGN KEY (channel_name) REFERENCES channels(channel_name) ON DELETE CASCADE
            )",
            [],
        )?;

        // Create index for efficient due schedule queries
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_scheduled_prompts_next_execution
             ON scheduled_prompts(next_execution_at)
             WHERE status = 'active'",
            [],
        )?;

        // Create index for listing by channel
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_scheduled_prompts_channel
             ON scheduled_prompts(channel_name)",
            [],
        )?;

        Ok(())
    }

    /// Create a new scheduled prompt
    pub fn create_schedule(&self, schedule: &ScheduledPrompt) -> Result<()> {
        let conn = self
            .db
            .lock()
            .map_err(|e| anyhow::anyhow!("Database mutex poisoned: {}", e))?;
        conn.execute(
            "INSERT INTO scheduled_prompts (
                id, channel_name, room_id, prompt, created_by, created_at,
                execute_at, cron_expression, last_executed_at, next_execution_at,
                status, error_message, execution_count
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            params![
                schedule.id,
                schedule.channel_name,
                schedule.room_id,
                schedule.prompt,
                schedule.created_by,
                schedule.created_at,
                schedule.execute_at,
                schedule.cron_expression,
                schedule.last_executed_at,
                schedule.next_execution_at,
                schedule.status.to_string(),
                schedule.error_message,
                schedule.execution_count,
            ],
        )?;
        Ok(())
    }

    /// Atomically claim and return schedules that are due for execution.
    /// Uses a claim token to ensure we only fetch schedules this call claimed,
    /// preventing race conditions with concurrent executions or crashed instances.
    pub fn claim_due_schedules(&self, now: DateTime<Utc>) -> Result<Vec<ScheduledPrompt>> {
        let conn = self
            .db
            .lock()
            .map_err(|e| anyhow::anyhow!("Database mutex poisoned: {}", e))?;
        let now_str = now.to_rfc3339();

        // Use now timestamp as claim token to identify schedules claimed by this call
        // This ensures we only fetch schedules we just marked, not pre-existing 'executing' ones
        conn.execute(
            "UPDATE scheduled_prompts
             SET status = 'executing', error_message = ?1
             WHERE status = 'active' AND next_execution_at <= ?2",
            params![now_str, now_str],
        )?;

        // Fetch only schedules claimed by this call (matching claim token in error_message)
        let mut stmt = conn.prepare(
            "SELECT id, channel_name, room_id, prompt, created_by, created_at,
                    execute_at, cron_expression, last_executed_at, next_execution_at,
                    status, error_message, execution_count
             FROM scheduled_prompts
             WHERE status = 'executing' AND error_message = ?1",
        )?;

        let schedules = stmt
            .query_map([&now_str], |row| {
                Ok(ScheduledPrompt {
                    id: row.get(0)?,
                    channel_name: row.get(1)?,
                    room_id: row.get(2)?,
                    prompt: row.get(3)?,
                    created_by: row.get(4)?,
                    created_at: row.get(5)?,
                    execute_at: row.get(6)?,
                    cron_expression: row.get(7)?,
                    last_executed_at: row.get(8)?,
                    next_execution_at: row.get(9)?,
                    status: ScheduleStatus::Executing,
                    error_message: None, // Clear claim token from returned struct
                    execution_count: row.get(12)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(schedules)
    }

    /// Mark a schedule as executed and update next execution time
    /// Resets status from 'executing' back to 'active' for recurring schedules
    pub fn mark_executed(&self, id: &str, next_execution: Option<DateTime<Utc>>) -> Result<()> {
        let conn = self
            .db
            .lock()
            .map_err(|e| anyhow::anyhow!("Database mutex poisoned: {}", e))?;
        let now = Utc::now().to_rfc3339();

        if let Some(next) = next_execution {
            // Recurring schedule - update next execution time and reset to active
            conn.execute(
                "UPDATE scheduled_prompts
                 SET last_executed_at = ?1,
                     next_execution_at = ?2,
                     status = 'active',
                     execution_count = execution_count + 1,
                     error_message = NULL
                 WHERE id = ?3",
                params![now, next.to_rfc3339(), id],
            )?;
        } else {
            // One-time schedule - mark as completed
            conn.execute(
                "UPDATE scheduled_prompts
                 SET last_executed_at = ?1,
                     status = 'completed',
                     execution_count = execution_count + 1,
                     error_message = NULL
                 WHERE id = ?2",
                params![now, id],
            )?;
        }

        Ok(())
    }

    /// Mark a schedule as failed
    pub fn mark_failed(&self, id: &str, error: &str) -> Result<()> {
        let conn = self
            .db
            .lock()
            .map_err(|e| anyhow::anyhow!("Database mutex poisoned: {}", e))?;
        conn.execute(
            "UPDATE scheduled_prompts
             SET status = 'failed', error_message = ?1
             WHERE id = ?2",
            params![error, id],
        )?;
        Ok(())
    }

    /// List all schedules
    pub fn list_all(&self) -> Result<Vec<ScheduledPrompt>> {
        let conn = self
            .db
            .lock()
            .map_err(|e| anyhow::anyhow!("Database mutex poisoned: {}", e))?;

        let mut stmt = conn.prepare(
            "SELECT id, channel_name, room_id, prompt, created_by, created_at,
                    execute_at, cron_expression, last_executed_at, next_execution_at,
                    status, error_message, execution_count
             FROM scheduled_prompts
             ORDER BY next_execution_at ASC",
        )?;

        let schedules = stmt
            .query_map([], |row| {
                Ok(ScheduledPrompt {
                    id: row.get(0)?,
                    channel_name: row.get(1)?,
                    room_id: row.get(2)?,
                    prompt: row.get(3)?,
                    created_by: row.get(4)?,
                    created_at: row.get(5)?,
                    execute_at: row.get(6)?,
                    cron_expression: row.get(7)?,
                    last_executed_at: row.get(8)?,
                    next_execution_at: row.get(9)?,
                    status: row
                        .get::<_, String>(10)?
                        .parse()
                        .unwrap_or(ScheduleStatus::Active),
                    error_message: row.get(11)?,
                    execution_count: row.get(12)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(schedules)
    }

    /// List schedules for a specific room
    pub fn list_by_room(&self, room_id: &str) -> Result<Vec<ScheduledPrompt>> {
        let conn = self
            .db
            .lock()
            .map_err(|e| anyhow::anyhow!("Database mutex poisoned: {}", e))?;

        let mut stmt = conn.prepare(
            "SELECT id, channel_name, room_id, prompt, created_by, created_at,
                    execute_at, cron_expression, last_executed_at, next_execution_at,
                    status, error_message, execution_count
             FROM scheduled_prompts
             WHERE room_id = ?1
             ORDER BY next_execution_at ASC",
        )?;

        let schedules = stmt
            .query_map([room_id], |row| {
                Ok(ScheduledPrompt {
                    id: row.get(0)?,
                    channel_name: row.get(1)?,
                    room_id: row.get(2)?,
                    prompt: row.get(3)?,
                    created_by: row.get(4)?,
                    created_at: row.get(5)?,
                    execute_at: row.get(6)?,
                    cron_expression: row.get(7)?,
                    last_executed_at: row.get(8)?,
                    next_execution_at: row.get(9)?,
                    status: row
                        .get::<_, String>(10)?
                        .parse()
                        .unwrap_or(ScheduleStatus::Active),
                    error_message: row.get(11)?,
                    execution_count: row.get(12)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(schedules)
    }

    /// Delete a schedule by ID
    pub fn delete_schedule(&self, id: &str) -> Result<bool> {
        let conn = self
            .db
            .lock()
            .map_err(|e| anyhow::anyhow!("Database mutex poisoned: {}", e))?;
        let rows = conn.execute("DELETE FROM scheduled_prompts WHERE id = ?1", params![id])?;
        Ok(rows > 0)
    }

    /// Pause a schedule
    pub fn pause_schedule(&self, id: &str) -> Result<bool> {
        let conn = self
            .db
            .lock()
            .map_err(|e| anyhow::anyhow!("Database mutex poisoned: {}", e))?;
        let rows = conn.execute(
            "UPDATE scheduled_prompts SET status = 'paused' WHERE id = ?1 AND status = 'active'",
            params![id],
        )?;
        Ok(rows > 0)
    }

    /// Resume a paused schedule
    pub fn resume_schedule(&self, id: &str) -> Result<bool> {
        let conn = self
            .db
            .lock()
            .map_err(|e| anyhow::anyhow!("Database mutex poisoned: {}", e))?;
        let rows = conn.execute(
            "UPDATE scheduled_prompts SET status = 'active' WHERE id = ?1 AND status = 'paused'",
            params![id],
        )?;
        Ok(rows > 0)
    }

    /// Get a schedule by ID
    pub fn get_by_id(&self, id: &str) -> Result<Option<ScheduledPrompt>> {
        let conn = self
            .db
            .lock()
            .map_err(|e| anyhow::anyhow!("Database mutex poisoned: {}", e))?;

        let mut stmt = conn.prepare(
            "SELECT id, channel_name, room_id, prompt, created_by, created_at,
                    execute_at, cron_expression, last_executed_at, next_execution_at,
                    status, error_message, execution_count
             FROM scheduled_prompts
             WHERE id = ?1",
        )?;

        let mut rows = stmt.query([id])?;
        match rows.next()? {
            Some(row) => Ok(Some(ScheduledPrompt {
                id: row.get(0)?,
                channel_name: row.get(1)?,
                room_id: row.get(2)?,
                prompt: row.get(3)?,
                created_by: row.get(4)?,
                created_at: row.get(5)?,
                execute_at: row.get(6)?,
                cron_expression: row.get(7)?,
                last_executed_at: row.get(8)?,
                next_execution_at: row.get(9)?,
                status: row
                    .get::<_, String>(10)?
                    .parse()
                    .unwrap_or(ScheduleStatus::Active),
                error_message: row.get(11)?,
                execution_count: row.get(12)?,
            })),
            None => Ok(None),
        }
    }

    /// Alias for get_by_id
    pub fn get_schedule(&self, id: &str) -> Result<Option<ScheduledPrompt>> {
        self.get_by_id(id)
    }

    /// List schedules by channel name
    pub fn list_by_channel(&self, channel_name: &str) -> Result<Vec<ScheduledPrompt>> {
        let conn = self
            .db
            .lock()
            .map_err(|e| anyhow::anyhow!("Database mutex poisoned: {}", e))?;

        let mut stmt = conn.prepare(
            "SELECT id, channel_name, room_id, prompt, created_by, created_at,
                    execute_at, cron_expression, last_executed_at, next_execution_at,
                    status, error_message, execution_count
             FROM scheduled_prompts
             WHERE channel_name = ?1
             ORDER BY next_execution_at ASC",
        )?;

        let rows = stmt.query_map([channel_name], |row| {
            Ok(ScheduledPrompt {
                id: row.get(0)?,
                channel_name: row.get(1)?,
                room_id: row.get(2)?,
                prompt: row.get(3)?,
                created_by: row.get(4)?,
                created_at: row.get(5)?,
                execute_at: row.get(6)?,
                cron_expression: row.get(7)?,
                last_executed_at: row.get(8)?,
                next_execution_at: row.get(9)?,
                status: row
                    .get::<_, String>(10)?
                    .parse()
                    .unwrap_or(ScheduleStatus::Active),
                error_message: row.get(11)?,
                execution_count: row.get(12)?,
            })
        })?;

        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| anyhow::anyhow!("Failed to collect schedules: {}", e))
    }

    /// Cancel a schedule (marks it as cancelled, doesn't delete)
    pub fn cancel_schedule(&self, id: &str) -> Result<bool> {
        let conn = self
            .db
            .lock()
            .map_err(|e| anyhow::anyhow!("Database mutex poisoned: {}", e))?;
        let rows = conn.execute(
            "UPDATE scheduled_prompts SET status = 'cancelled' WHERE id = ?1",
            params![id],
        )?;
        Ok(rows > 0)
    }
}

// Background scheduler execution module
use crate::{
    claude,
    config::Config,
    session::{Channel, SessionStore},
    utils::{chunk_message, expand_slash_command, log_matrix_message, markdown_to_html, MAX_CHUNK_SIZE},
};
use matrix_sdk::{ruma::events::room::message::RoomMessageEventContent, Client};
use std::path::Path;
use std::time::Duration as StdDuration;
use tokio::time::interval;

/// Write context file for MCP tools (used by scheduler before Claude invocation)
async fn write_context_file(channel: &Channel) -> Result<()> {
    let gorp_dir = Path::new(&channel.directory).join(".gorp");
    tokio::fs::create_dir_all(&gorp_dir).await?;

    let context = serde_json::json!({
        "room_id": channel.room_id,
        "channel_name": channel.channel_name,
        "session_id": channel.session_id,
        "updated_at": Utc::now().to_rfc3339()
    });

    let context_path = gorp_dir.join("context.json");
    tokio::fs::write(&context_path, serde_json::to_string_pretty(&context)?).await?;

    tracing::debug!(path = %context_path.display(), "Wrote MCP context file for scheduled task");
    Ok(())
}

/// Start the background scheduler task that checks for and executes due schedules
pub async fn start_scheduler(
    scheduler_store: SchedulerStore,
    session_store: SessionStore,
    client: Client,
    config: Arc<Config>,
    check_interval: StdDuration,
) {
    tracing::info!(
        interval_secs = check_interval.as_secs(),
        "Starting scheduler background task"
    );

    let mut ticker = interval(check_interval);

    loop {
        ticker.tick().await;

        let now = Utc::now();
        // Use claim_due_schedules to atomically mark schedules as 'executing'
        // This prevents race conditions where a slow execution could cause duplicates
        match scheduler_store.claim_due_schedules(now) {
            Ok(schedules) => {
                if !schedules.is_empty() {
                    tracing::info!(
                        count = schedules.len(),
                        "Claimed due schedules for execution"
                    );
                }

                for schedule in schedules {
                    // Clone what we need for the spawned task
                    let store = scheduler_store.clone();
                    let sess_store = session_store.clone();
                    let cli = client.clone();
                    let cfg = Arc::clone(&config);

                    // Execute each due schedule concurrently
                    tokio::spawn(async move {
                        execute_schedule(schedule, store, sess_store, cli, cfg).await;
                    });
                }
            }
            Err(e) => {
                tracing::error!(error = %e, "Failed to fetch due schedules");
            }
        }
    }
}

/// Execute a single scheduled prompt
async fn execute_schedule(
    schedule: ScheduledPrompt,
    scheduler_store: SchedulerStore,
    session_store: SessionStore,
    client: Client,
    config: Arc<Config>,
) {
    let prompt_preview: String = schedule.prompt.chars().take(50).collect();
    tracing::info!(
        schedule_id = %schedule.id,
        channel = %schedule.channel_name,
        prompt_preview = %prompt_preview,
        "Executing scheduled prompt"
    );

    // Get channel info
    let channel = match session_store.get_by_name(&schedule.channel_name) {
        Ok(Some(c)) => c,
        Ok(None) => {
            tracing::error!(
                schedule_id = %schedule.id,
                channel = %schedule.channel_name,
                "Channel no longer exists"
            );
            let _ = scheduler_store.mark_failed(&schedule.id, "Channel no longer exists");
            return;
        }
        Err(e) => {
            tracing::error!(
                schedule_id = %schedule.id,
                error = %e,
                "Failed to get channel"
            );
            let _ = scheduler_store.mark_failed(&schedule.id, &e.to_string());
            return;
        }
    };

    // Get Matrix room
    let room_id: matrix_sdk::ruma::OwnedRoomId = match schedule.room_id.parse() {
        Ok(id) => id,
        Err(e) => {
            tracing::error!(
                schedule_id = %schedule.id,
                room_id = %schedule.room_id,
                error = %e,
                "Invalid room ID"
            );
            let _ = scheduler_store.mark_failed(&schedule.id, &format!("Invalid room ID: {}", e));
            return;
        }
    };

    let Some(room) = client.get_room(&room_id) else {
        tracing::error!(
            schedule_id = %schedule.id,
            room_id = %schedule.room_id,
            "Room not found"
        );
        let _ = scheduler_store.mark_failed(&schedule.id, "Room not found");
        return;
    };

    // Send notification that scheduled prompt is executing
    let notification = format!(
        "⏰ **Scheduled Task**\n> {}",
        schedule.prompt.chars().take(200).collect::<String>()
    );
    let notification_html = markdown_to_html(&notification);
    if let Err(e) = room
        .send(RoomMessageEventContent::text_html(
            &notification,
            &notification_html,
        ))
        .await
    {
        tracing::warn!(error = %e, "Failed to send schedule notification");
    }

    // Write context file for MCP tools before invoking Claude
    if let Err(e) = write_context_file(&channel).await {
        tracing::warn!(error = %e, "Failed to write context file for scheduled task");
        // Non-fatal - continue without context file
    }

    // Expand slash commands at execution time (so updates to commands are picked up)
    let prompt = match expand_slash_command(&schedule.prompt, &channel.directory) {
        Ok(p) => p,
        Err(e) => {
            tracing::error!(
                schedule_id = %schedule.id,
                error = %e,
                "Failed to expand slash command"
            );
            let error_msg = format!("⚠️ Scheduled task failed: {}", e);
            let _ = room
                .send(RoomMessageEventContent::text_plain(&error_msg))
                .await;
            let _ = scheduler_store.mark_failed(&schedule.id, &e);
            return;
        }
    };

    // Start typing indicator
    let _ = room.typing_notice(true).await;

    // Use streaming mode to capture actual response even if session ends with errors
    let mut rx = match claude::invoke_claude_streaming(
        &config.claude.binary_path,
        config.claude.sdk_url.as_deref(),
        channel.cli_args(),
        &prompt,
        Some(&channel.directory),
    )
    .await
    {
        Ok(rx) => rx,
        Err(e) => {
            let _ = room.typing_notice(false).await;
            tracing::error!(
                schedule_id = %schedule.id,
                error = %e,
                "Failed to spawn Claude for scheduled task"
            );
            let error_msg = format!("⚠️ Scheduled task failed: {}", e);
            let _ = room
                .send(RoomMessageEventContent::text_plain(&error_msg))
                .await;
            let _ = scheduler_store.mark_failed(&schedule.id, &e.to_string());
            return;
        }
    };

    // Collect response from stream
    let mut response = String::new();
    let mut had_error = false;

    while let Some(event) = rx.recv().await {
        match event {
            claude::ClaudeEvent::ToolUse { name, input_preview } => {
                tracing::debug!(tool = %name, preview = %input_preview, "Scheduled task tool use");
            }
            claude::ClaudeEvent::Result { text, usage } => {
                // Record token usage metrics
                metrics::record_claude_tokens(
                    usage.input_tokens,
                    usage.output_tokens,
                    usage.cache_read_tokens,
                    usage.cache_creation_tokens,
                );
                // Convert dollars to cents and record
                let cost_cents = (usage.total_cost_usd * 100.0).round() as u64;
                metrics::record_claude_cost_cents(cost_cents);

                tracing::info!(
                    input_tokens = usage.input_tokens,
                    output_tokens = usage.output_tokens,
                    cost_usd = usage.total_cost_usd,
                    "Scheduled task usage recorded"
                );

                response = text;
            }
            claude::ClaudeEvent::Error(e) => {
                tracing::warn!(error = %e, "Scheduled task Claude error");
                had_error = true;
                // Don't return yet - we might have captured text before the error
            }
            claude::ClaudeEvent::OrphanedSession => {
                tracing::warn!("Scheduled task hit orphaned session");
                // Reset the session so future executions start fresh
                if let Err(e) = session_store.reset_orphaned_session(&channel.room_id) {
                    tracing::error!(error = %e, "Failed to reset orphaned session in scheduler");
                }
                let _ = room
                    .send(RoomMessageEventContent::text_plain(
                        "🔄 Session was reset (conversation data was lost). Scheduled task will retry next time.",
                    ))
                    .await;
                let _ = scheduler_store.mark_failed(&schedule.id, "Session was orphaned");
                return;
            }
        }
    }

    // Stop typing
    let _ = room.typing_notice(false).await;

    // Check for empty response
    if response.trim().is_empty() {
        if had_error {
            tracing::error!(
                schedule_id = %schedule.id,
                prompt = %schedule.prompt,
                "Claude returned empty response with error for scheduled task"
            );
            let error_msg = "⚠️ Scheduled task failed: Claude encountered an error and returned no response.";
            let _ = room
                .send(RoomMessageEventContent::text_plain(error_msg))
                .await;
            let _ = scheduler_store.mark_failed(&schedule.id, "Claude error with empty response");
        } else {
            tracing::error!(
                schedule_id = %schedule.id,
                prompt = %schedule.prompt,
                "Claude returned empty response for scheduled task"
            );
            let error_msg = "⚠️ Scheduled task failed: Claude returned an empty response. This may indicate a session issue or prompt problem.";
            let _ = room
                .send(RoomMessageEventContent::text_plain(error_msg))
                .await;
            let _ = scheduler_store.mark_failed(&schedule.id, "Empty response from Claude");
        }
        return;
    }

    // Send response to room with chunking
    let chunks = chunk_message(&response, MAX_CHUNK_SIZE);
    let chunk_count = chunks.len();

    for (i, chunk) in chunks.into_iter().enumerate() {
        let html = markdown_to_html(&chunk);
        if let Err(e) = room
            .send(RoomMessageEventContent::text_html(&chunk, &html))
            .await
        {
            tracing::warn!(error = %e, chunk = i, "Failed to send response chunk");
        }

        // Log the Matrix message
        log_matrix_message(
            &channel.directory,
            room.room_id().as_str(),
            "scheduled_response",
            &chunk,
            Some(&html),
            if chunk_count > 1 { Some(i) } else { None },
            if chunk_count > 1 {
                Some(chunk_count)
            } else {
                None
            },
        )
        .await;

        // Small delay between chunks
        if i < chunk_count - 1 {
            tokio::time::sleep(StdDuration::from_millis(100)).await;
        }
    }

    // Calculate next execution for recurring schedules
    let next_execution = if let Some(ref cron_expr) = schedule.cron_expression {
        match compute_next_cron_execution_in_tz(cron_expr, &config.scheduler.timezone) {
            Ok(next) => Some(next),
            Err(e) => {
                // Log the error and mark schedule as failed instead of silently completing
                tracing::error!(
                    schedule_id = %schedule.id,
                    cron = %cron_expr,
                    timezone = %config.scheduler.timezone,
                    error = %e,
                    "Failed to compute next execution time for recurring schedule"
                );
                let _ = scheduler_store.mark_failed(
                    &schedule.id,
                    &format!("Failed to compute next execution: {}", e),
                );
                return; // Exit early - don't mark as executed
            }
        }
    } else {
        None // One-time schedule - will be marked completed
    };

    if let Err(e) = scheduler_store.mark_executed(&schedule.id, next_execution) {
        tracing::error!(
            schedule_id = %schedule.id,
            error = %e,
            "Failed to mark schedule as executed"
        );
    } else {
        // Record successful execution metric here (after we know it worked)
        metrics::record_schedule_executed();
        let status = if next_execution.is_some() {
            "rescheduled"
        } else {
            "completed"
        };
        tracing::info!(
            schedule_id = %schedule.id,
            status,
            "Schedule execution successful"
        );
    }

    // Log warning if there was an error but we still got a response
    if had_error {
        tracing::warn!(
            schedule_id = %schedule.id,
            "Scheduled task completed with warnings (Claude encountered non-fatal errors)"
        );
    }
}
