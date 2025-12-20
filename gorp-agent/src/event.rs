// ABOUTME: Event types emitted by agent backends during prompt execution.
// ABOUTME: Includes tool lifecycle, results, errors, and extensibility via Custom variant.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Events emitted by agent backends during prompt execution
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum AgentEvent {
    /// Streaming text chunk for real-time display
    Text(String),

    /// Tool started execution
    ToolStart {
        /// Unique identifier for this tool invocation
        id: String,
        /// Tool name (e.g., "Read", "Bash", "Edit")
        name: String,
        /// Full input passed to the tool
        input: Value,
    },

    /// Tool progress update (backend-specific)
    ToolProgress {
        /// Matches the id from ToolStart
        id: String,
        /// Backend-specific progress data
        update: Value,
    },

    /// Tool completed execution
    ToolEnd {
        /// Matches the id from ToolStart
        id: String,
        /// Tool name
        name: String,
        /// Full output from the tool
        output: Value,
        /// Whether the tool succeeded
        success: bool,
        /// Execution time in milliseconds
        duration_ms: u64,
    },

    /// Final result with optional usage statistics
    Result {
        /// The final text response
        text: String,
        /// Token usage and cost (if available)
        usage: Option<Usage>,
        /// Backend-specific metadata
        metadata: Value,
    },

    /// Error occurred during execution
    Error {
        /// Typed error code for programmatic handling
        code: ErrorCode,
        /// Human-readable error message
        message: String,
        /// Whether the error is recoverable (can retry)
        recoverable: bool,
    },

    /// Session is invalid and needs to be recreated
    SessionInvalid {
        /// Reason the session became invalid
        reason: String,
    },

    /// Backend forced creation of a new session
    SessionChanged {
        /// The new session ID to use
        new_session_id: String,
    },

    /// Backend-specific event for extensibility
    Custom {
        /// Event kind (e.g., "acp.thought_chunk", "openai.run_step")
        kind: String,
        /// Event payload
        payload: Value,
    },
}

/// Typed error codes for programmatic handling
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ErrorCode {
    /// Request timed out
    Timeout,
    /// Rate limited by the backend
    RateLimited,
    /// Authentication failed
    AuthFailed,
    /// Session no longer exists
    SessionOrphaned,
    /// Tool execution failed
    ToolFailed,
    /// Permission denied for operation
    PermissionDenied,
    /// Backend-specific error
    BackendError,
    /// Unknown error
    Unknown,
}

/// Token usage and cost tracking
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Usage {
    /// Input tokens consumed
    pub input_tokens: u64,
    /// Output tokens generated
    pub output_tokens: u64,
    /// Tokens read from cache
    pub cache_read_tokens: Option<u64>,
    /// Tokens written to cache
    pub cache_write_tokens: Option<u64>,
    /// Total cost in USD
    pub cost_usd: Option<f64>,
    /// Backend-specific usage data
    pub extra: Option<Value>,
}
