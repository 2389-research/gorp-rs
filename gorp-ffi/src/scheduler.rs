// ABOUTME: FFI wrapper for SchedulerStore.
// ABOUTME: Provides cron-based prompt scheduling.

use crate::error::FfiError;
use crate::session::FfiSessionStore;
use chrono::Utc;
use gorp_core::scheduler::{
    parse_time_expression, ParsedSchedule, ScheduleStatus, ScheduledPrompt, SchedulerStore,
};
use std::sync::Arc;

/// FFI-safe schedule status
#[derive(Debug, Clone, Copy, uniffi::Enum)]
pub enum FfiScheduleStatus {
    Active,
    Paused,
    Completed,
    Failed,
    Executing,
    Cancelled,
}

impl From<ScheduleStatus> for FfiScheduleStatus {
    fn from(s: ScheduleStatus) -> Self {
        match s {
            ScheduleStatus::Active => FfiScheduleStatus::Active,
            ScheduleStatus::Paused => FfiScheduleStatus::Paused,
            ScheduleStatus::Completed => FfiScheduleStatus::Completed,
            ScheduleStatus::Failed => FfiScheduleStatus::Failed,
            ScheduleStatus::Executing => FfiScheduleStatus::Executing,
            ScheduleStatus::Cancelled => FfiScheduleStatus::Cancelled,
        }
    }
}

/// FFI-safe scheduled prompt record
#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiScheduledPrompt {
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
    pub status: FfiScheduleStatus,
    pub error_message: Option<String>,
    pub execution_count: i32,
}

impl From<ScheduledPrompt> for FfiScheduledPrompt {
    fn from(s: ScheduledPrompt) -> Self {
        Self {
            id: s.id,
            channel_name: s.channel_name,
            room_id: s.room_id,
            prompt: s.prompt,
            created_by: s.created_by,
            created_at: s.created_at,
            execute_at: s.execute_at,
            cron_expression: s.cron_expression,
            last_executed_at: s.last_executed_at,
            next_execution_at: s.next_execution_at,
            status: s.status.into(),
            error_message: s.error_message,
            execution_count: s.execution_count,
        }
    }
}

/// FFI wrapper for SchedulerStore
#[derive(uniffi::Object)]
pub struct FfiSchedulerStore {
    inner: SchedulerStore,
}

#[uniffi::export]
impl FfiSchedulerStore {
    /// Create a scheduler store that shares the session store's database
    #[uniffi::constructor]
    pub fn new(session_store: &FfiSessionStore) -> Result<Arc<Self>, FfiError> {
        let db = session_store.inner().db_connection();
        let store = SchedulerStore::new(db);
        store
            .initialize_schema()
            .map_err(|e| FfiError::DatabaseError(e.to_string()))?;
        Ok(Arc::new(Self { inner: store }))
    }

    /// List all scheduled prompts
    pub fn list_all(&self) -> Result<Vec<FfiScheduledPrompt>, FfiError> {
        let schedules = self
            .inner
            .list_all()
            .map_err(|e| FfiError::DatabaseError(e.to_string()))?;
        Ok(schedules.into_iter().map(Into::into).collect())
    }

    /// List schedules for a specific room
    pub fn list_by_room(&self, room_id: String) -> Result<Vec<FfiScheduledPrompt>, FfiError> {
        let schedules = self
            .inner
            .list_by_room(&room_id)
            .map_err(|e| FfiError::DatabaseError(e.to_string()))?;
        Ok(schedules.into_iter().map(Into::into).collect())
    }

    /// List schedules for a specific channel
    pub fn list_by_channel(
        &self,
        channel_name: String,
    ) -> Result<Vec<FfiScheduledPrompt>, FfiError> {
        let schedules = self
            .inner
            .list_by_channel(&channel_name)
            .map_err(|e| FfiError::DatabaseError(e.to_string()))?;
        Ok(schedules.into_iter().map(Into::into).collect())
    }

    /// Get a schedule by ID
    pub fn get_by_id(&self, id: String) -> Result<Option<FfiScheduledPrompt>, FfiError> {
        let schedule = self
            .inner
            .get_by_id(&id)
            .map_err(|e| FfiError::DatabaseError(e.to_string()))?;
        Ok(schedule.map(Into::into))
    }

    /// Delete a schedule by ID
    pub fn delete_schedule(&self, id: String) -> Result<bool, FfiError> {
        self.inner
            .delete_schedule(&id)
            .map_err(|e| FfiError::DatabaseError(e.to_string()))
    }

    /// Pause a schedule
    pub fn pause_schedule(&self, id: String) -> Result<bool, FfiError> {
        self.inner
            .pause_schedule(&id)
            .map_err(|e| FfiError::DatabaseError(e.to_string()))
    }

    /// Resume a paused schedule
    pub fn resume_schedule(&self, id: String) -> Result<bool, FfiError> {
        self.inner
            .resume_schedule(&id)
            .map_err(|e| FfiError::DatabaseError(e.to_string()))
    }

    /// Cancel a schedule
    pub fn cancel_schedule(&self, id: String) -> Result<bool, FfiError> {
        self.inner
            .cancel_schedule(&id)
            .map_err(|e| FfiError::DatabaseError(e.to_string()))
    }

    /// Create a new scheduled prompt
    ///
    /// Time expression can be:
    /// - Cron expression: "0 9 * * *" (daily at 9am)
    /// - Relative time: "in 5 minutes", "in 2 hours"
    /// - Absolute time: "at 3pm", "at 14:30"
    pub fn create_schedule(
        &self,
        channel_name: String,
        room_id: String,
        prompt: String,
        created_by: String,
        time_expression: String,
        timezone: String,
    ) -> Result<FfiScheduledPrompt, FfiError> {
        // Parse the time expression
        let parsed = parse_time_expression(&time_expression, &timezone)
            .map_err(|e| FfiError::InvalidInput(format!("Invalid time expression: {}", e)))?;

        let now = Utc::now();
        let created_at = now.to_rfc3339();

        // Build the scheduled prompt based on parsed result
        let (execute_at, cron_expression, next_execution_at) = match parsed {
            ParsedSchedule::OneTime(dt) => {
                let dt_str = dt.to_rfc3339();
                (Some(dt_str.clone()), None, dt_str)
            }
            ParsedSchedule::Recurring { cron, next } => {
                let next_str = next.to_rfc3339();
                (None, Some(cron), next_str)
            }
        };

        let schedule = ScheduledPrompt {
            id: uuid::Uuid::new_v4().to_string(),
            channel_name,
            room_id,
            prompt,
            created_by,
            created_at,
            execute_at,
            cron_expression,
            last_executed_at: None,
            next_execution_at,
            status: ScheduleStatus::Active,
            error_message: None,
            execution_count: 0,
        };

        self.inner
            .create_schedule(&schedule)
            .map_err(|e| FfiError::DatabaseError(e.to_string()))?;

        Ok(schedule.into())
    }
}
