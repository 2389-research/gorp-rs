// ABOUTME: Scheduler execution for gorp, publishing prompts to the message bus.
// ABOUTME: Re-exports core scheduler types from gorp-core, publishes BusMessage when schedules fire.

// Re-export all core scheduler types and functions from gorp-core
// This ensures type consistency across the codebase
pub use gorp_core::scheduler::{
    compute_next_cron_execution, compute_next_cron_execution_in_tz, parse_time_expression,
    ParsedSchedule, ScheduleStatus, ScheduledPrompt, SchedulerCallback, SchedulerStore,
};

use anyhow::Result;
use chrono::Utc;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration as StdDuration;
use tokio::time::interval;

use crate::{
    bus::{BusMessage, MessageBus, MessageSource, SessionTarget},
    config::Config,
    metrics,
    session::{Channel, SessionStore},
    utils::expand_slash_command,
    warm_session::{prepare_session_async, SharedWarmSessionManager},
};

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

/// Start the background scheduler task that checks for and executes due schedules.
///
/// When a schedule fires, the scheduler publishes a `BusMessage` to the message bus.
/// The orchestrator handles routing the message to the appropriate agent session,
/// and gateway adapters handle delivering responses to connected platforms.
pub async fn start_scheduler(
    scheduler_store: SchedulerStore,
    session_store: SessionStore,
    bus: Arc<MessageBus>,
    config: Arc<Config>,
    check_interval: StdDuration,
    warm_manager: SharedWarmSessionManager,
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
                    let bus_clone = Arc::clone(&bus);
                    let cfg = Arc::clone(&config);

                    // Execute each due schedule concurrently
                    // Publishing to the bus is Send-safe, so tokio::spawn works
                    tokio::spawn(async move {
                        execute_schedule(schedule, store, sess_store, bus_clone, cfg).await;
                    });
                }
            }
            Err(e) => {
                tracing::error!(error = %e, "Failed to fetch due schedules");
            }
        }

        // Check for schedules that should be pre-warmed
        // Pre-warm if within pre_warm_secs of execution
        let pre_warm_duration = chrono::Duration::seconds(config.backend.pre_warm_secs as i64);
        let pre_warm_cutoff = now + pre_warm_duration;

        if let Ok(all_schedules) = scheduler_store.list_all() {
            for schedule in all_schedules {
                // Only pre-warm active schedules
                if schedule.status != ScheduleStatus::Active {
                    continue;
                }

                // Parse next execution time
                if let Ok(next_exec) =
                    chrono::DateTime::parse_from_rfc3339(&schedule.next_execution_at)
                {
                    let next_exec_utc = next_exec.with_timezone(&Utc);

                    // Pre-warm if within the window
                    if next_exec_utc > now && next_exec_utc <= pre_warm_cutoff {
                        // Get channel for this schedule
                        if let Ok(Some(channel)) = session_store.get_by_name(&schedule.channel_name)
                        {
                            let channel_name = schedule.channel_name.clone();

                            // Pre-warm using prepare_session_async which minimizes lock holding
                            // This allows concurrent pre-warming without blocking other channels
                            match prepare_session_async(&warm_manager, &channel).await {
                                Ok(_) => {
                                    tracing::debug!(
                                        channel = %channel_name,
                                        "Pre-warmed session for upcoming schedule"
                                    );
                                }
                                Err(e) => {
                                    tracing::warn!(
                                        channel = %channel_name,
                                        error = %e,
                                        "Pre-warm failed for upcoming schedule"
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Execute a single scheduled prompt by publishing a BusMessage to the message bus.
///
/// The scheduler handles: channel lookup, context file writing, slash command expansion,
/// and schedule lifecycle (marking executed/failed, computing next execution).
/// The orchestrator handles: agent session management and response streaming.
/// Gateway adapters handle: delivering responses to connected platforms.
async fn execute_schedule(
    schedule: ScheduledPrompt,
    scheduler_store: SchedulerStore,
    session_store: SessionStore,
    bus: Arc<MessageBus>,
    config: Arc<Config>,
) {
    let prompt_preview: String = schedule.prompt.chars().take(50).collect();
    tracing::info!(
        schedule_id = %schedule.id,
        channel = %schedule.channel_name,
        prompt_preview = %prompt_preview,
        "Executing scheduled prompt via message bus"
    );

    // Get channel info (needed for directory, context file, slash command expansion)
    let channel = match session_store.get_by_name(&schedule.channel_name) {
        Ok(Some(c)) => c,
        Ok(None) => {
            tracing::error!(
                schedule_id = %schedule.id,
                channel = %schedule.channel_name,
                "Channel no longer exists"
            );
            if let Err(e) = scheduler_store.mark_failed(&schedule.id, "Channel no longer exists") {
                tracing::error!(error = %e, schedule_id = %schedule.id, "Failed to mark schedule failed");
            }
            return;
        }
        Err(e) => {
            tracing::error!(
                schedule_id = %schedule.id,
                error = %e,
                "Failed to get channel"
            );
            if let Err(e) = scheduler_store.mark_failed(&schedule.id, &e.to_string()) {
                tracing::error!(error = %e, schedule_id = %schedule.id, "Failed to mark schedule failed");
            }
            return;
        }
    };

    // Write context file for MCP tools before publishing to the bus
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
            if let Err(e) = scheduler_store.mark_failed(&schedule.id, &e.to_string()) {
                tracing::error!(error = %e, schedule_id = %schedule.id, "Failed to mark schedule failed");
            }
            return;
        }
    };

    // Publish a BusMessage so the orchestrator routes it to the agent session
    let msg = BusMessage {
        id: format!("sched-{}-{}", schedule.id, schedule.execution_count),
        source: MessageSource::Api {
            token_hint: "scheduler".to_string(),
        },
        session_target: SessionTarget::Session {
            name: schedule.channel_name.clone(),
        },
        sender: if schedule.created_by.is_empty() {
            "scheduler".to_string()
        } else {
            schedule.created_by.clone()
        },
        body: prompt,
        timestamp: Utc::now(),
    };

    tracing::info!(
        schedule_id = %schedule.id,
        bus_msg_id = %msg.id,
        channel = %schedule.channel_name,
        "Publishing scheduled prompt to message bus"
    );
    bus.publish_inbound(msg);

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
                if let Err(e) = scheduler_store.mark_failed(
                    &schedule.id,
                    &format!("Failed to compute next execution: {}", e),
                ) {
                    tracing::error!(error = %e, schedule_id = %schedule.id, "Failed to mark schedule failed");
                }
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
}
