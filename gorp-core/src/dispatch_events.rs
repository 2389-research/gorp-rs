// ABOUTME: Event types for DISPATCH control plane communication.
// ABOUTME: Workers emit these events which gorp routes to DISPATCH.

use serde::{Deserialize, Serialize};

/// Events that workers can emit to DISPATCH
///
/// These are serialized and stored in the dispatch_events table,
/// then routed to the DISPATCH agent for processing.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WorkerEvent {
    /// Task completed successfully
    TaskCompleted {
        room_id: String,
        task_id: Option<String>,
        summary: String,
    },
    /// Task failed with an error
    TaskFailed {
        room_id: String,
        task_id: Option<String>,
        error: String,
    },
    /// Agent is waiting for user input
    WaitingForInput { room_id: String, question: String },
    /// Progress update during long-running work
    ProgressUpdate {
        room_id: String,
        message: String,
        progress: Option<u32>, // 0-100 percent
    },
}

impl WorkerEvent {
    /// Get the source room ID
    pub fn room_id(&self) -> &str {
        match self {
            Self::TaskCompleted { room_id, .. } => room_id,
            Self::TaskFailed { room_id, .. } => room_id,
            Self::WaitingForInput { room_id, .. } => room_id,
            Self::ProgressUpdate { room_id, .. } => room_id,
        }
    }

    /// Get the event type name
    pub fn event_type(&self) -> &'static str {
        match self {
            Self::TaskCompleted { .. } => "task_completed",
            Self::TaskFailed { .. } => "task_failed",
            Self::WaitingForInput { .. } => "waiting_for_input",
            Self::ProgressUpdate { .. } => "progress_update",
        }
    }

    /// Get the task ID if this event is related to a dispatched task
    pub fn task_id(&self) -> Option<&str> {
        match self {
            Self::TaskCompleted { task_id, .. } => task_id.as_deref(),
            Self::TaskFailed { task_id, .. } => task_id.as_deref(),
            _ => None,
        }
    }

    /// Check if this is a high-priority event (errors, questions)
    pub fn is_high_priority(&self) -> bool {
        matches!(self, Self::TaskFailed { .. } | Self::WaitingForInput { .. })
    }

    /// Create a TaskCompleted event
    pub fn task_completed(
        room_id: impl Into<String>,
        task_id: Option<String>,
        summary: impl Into<String>,
    ) -> Self {
        Self::TaskCompleted {
            room_id: room_id.into(),
            task_id,
            summary: summary.into(),
        }
    }

    /// Create a TaskFailed event
    pub fn task_failed(
        room_id: impl Into<String>,
        task_id: Option<String>,
        error: impl Into<String>,
    ) -> Self {
        Self::TaskFailed {
            room_id: room_id.into(),
            task_id,
            error: error.into(),
        }
    }

    /// Create a WaitingForInput event
    pub fn waiting_for_input(room_id: impl Into<String>, question: impl Into<String>) -> Self {
        Self::WaitingForInput {
            room_id: room_id.into(),
            question: question.into(),
        }
    }

    /// Create a ProgressUpdate event
    pub fn progress_update(
        room_id: impl Into<String>,
        message: impl Into<String>,
        progress: Option<u32>,
    ) -> Self {
        Self::ProgressUpdate {
            room_id: room_id.into(),
            message: message.into(),
            progress,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_serialization() {
        let event =
            WorkerEvent::task_completed("!room:example.com", Some("task-123".into()), "Done!");
        let json = serde_json::to_string(&event).unwrap();

        assert!(json.contains("task_completed"));
        assert!(json.contains("!room:example.com"));
        assert!(json.contains("Done!"));
    }

    #[test]
    fn test_event_deserialization() {
        let json = r#"{"type":"task_failed","room_id":"!room:example.com","task_id":null,"error":"Something went wrong"}"#;
        let event: WorkerEvent = serde_json::from_str(json).unwrap();

        assert!(matches!(event, WorkerEvent::TaskFailed { .. }));
        assert_eq!(event.room_id(), "!room:example.com");
    }

    #[test]
    fn test_room_id_accessor() {
        let event = WorkerEvent::waiting_for_input("!test:example.com", "What now?");
        assert_eq!(event.room_id(), "!test:example.com");
    }

    #[test]
    fn test_high_priority() {
        assert!(WorkerEvent::task_failed("!r", None, "err").is_high_priority());
        assert!(WorkerEvent::waiting_for_input("!r", "q").is_high_priority());
        assert!(!WorkerEvent::task_completed("!r", None, "done").is_high_priority());
        assert!(!WorkerEvent::progress_update("!r", "msg", None).is_high_priority());
    }

    #[test]
    fn test_task_id_accessor() {
        let with_task = WorkerEvent::task_completed("!r", Some("t1".into()), "done");
        let without_task = WorkerEvent::task_completed("!r", None, "done");
        let progress = WorkerEvent::progress_update("!r", "msg", None);

        assert_eq!(with_task.task_id(), Some("t1"));
        assert_eq!(without_task.task_id(), None);
        assert_eq!(progress.task_id(), None);
    }
}
