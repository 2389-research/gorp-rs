// ABOUTME: FFI-safe event types and callback interface.
// ABOUTME: Streams agent events to Swift/Kotlin via callbacks.

use gorp_agent::AgentEvent;

/// FFI-safe error codes matching gorp-agent::ErrorCode
#[derive(Debug, Clone, Copy, uniffi::Enum)]
pub enum FfiErrorCode {
    Timeout,
    RateLimited,
    AuthFailed,
    SessionOrphaned,
    ToolFailed,
    PermissionDenied,
    BackendError,
    Unknown,
}

impl From<gorp_agent::ErrorCode> for FfiErrorCode {
    fn from(code: gorp_agent::ErrorCode) -> Self {
        match code {
            gorp_agent::ErrorCode::Timeout => FfiErrorCode::Timeout,
            gorp_agent::ErrorCode::RateLimited => FfiErrorCode::RateLimited,
            gorp_agent::ErrorCode::AuthFailed => FfiErrorCode::AuthFailed,
            gorp_agent::ErrorCode::SessionOrphaned => FfiErrorCode::SessionOrphaned,
            gorp_agent::ErrorCode::ToolFailed => FfiErrorCode::ToolFailed,
            gorp_agent::ErrorCode::PermissionDenied => FfiErrorCode::PermissionDenied,
            gorp_agent::ErrorCode::BackendError => FfiErrorCode::BackendError,
            gorp_agent::ErrorCode::Unknown => FfiErrorCode::Unknown,
        }
    }
}

/// FFI-safe usage statistics
#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: Option<u64>,
    pub cache_write_tokens: Option<u64>,
    pub cost_usd: Option<f64>,
    /// Backend-specific usage data as JSON string
    pub extra_json: Option<String>,
}

impl From<gorp_agent::Usage> for FfiUsage {
    fn from(u: gorp_agent::Usage) -> Self {
        Self {
            input_tokens: u.input_tokens,
            output_tokens: u.output_tokens,
            cache_read_tokens: u.cache_read_tokens,
            cache_write_tokens: u.cache_write_tokens,
            cost_usd: u.cost_usd,
            extra_json: u.extra.map(|v| v.to_string()),
        }
    }
}

/// Callback interface implemented by Swift/Kotlin
#[uniffi::export(callback_interface)]
pub trait AgentEventCallback: Send + Sync {
    fn on_text(&self, text: String);
    fn on_tool_start(&self, id: String, name: String, input_json: String);
    fn on_tool_progress(&self, id: String, update_json: String);
    fn on_tool_end(
        &self,
        id: String,
        name: String,
        output_json: String,
        success: bool,
        duration_ms: u64,
    );
    fn on_result(&self, text: String, usage: Option<FfiUsage>, metadata_json: String);
    fn on_error(&self, code: FfiErrorCode, message: String, recoverable: bool);
    fn on_session_invalid(&self, reason: String);
    fn on_session_changed(&self, new_session_id: String);
    fn on_custom(&self, kind: String, payload_json: String);
}

/// Dispatch a gorp-agent event to the callback
pub fn dispatch_event(callback: &dyn AgentEventCallback, event: AgentEvent) {
    match event {
        AgentEvent::Text(text) => callback.on_text(text),
        AgentEvent::ToolStart { id, name, input } => {
            callback.on_tool_start(id, name, input.to_string());
        }
        AgentEvent::ToolProgress { id, update } => {
            callback.on_tool_progress(id, update.to_string());
        }
        AgentEvent::ToolEnd {
            id,
            name,
            output,
            success,
            duration_ms,
        } => {
            callback.on_tool_end(id, name, output.to_string(), success, duration_ms);
        }
        AgentEvent::Result {
            text,
            usage,
            metadata,
        } => {
            callback.on_result(text, usage.map(Into::into), metadata.to_string());
        }
        AgentEvent::Error {
            code,
            message,
            recoverable,
        } => {
            callback.on_error(code.into(), message, recoverable);
        }
        AgentEvent::SessionInvalid { reason } => {
            callback.on_session_invalid(reason);
        }
        AgentEvent::SessionChanged { new_session_id } => {
            callback.on_session_changed(new_session_id);
        }
        AgentEvent::Custom { kind, payload } => {
            callback.on_custom(kind, payload.to_string());
        }
    }
}
