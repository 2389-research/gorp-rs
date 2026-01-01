// ABOUTME: FFI-safe error types for gorp-ffi.
// ABOUTME: Maps internal errors to UniFFI-compatible enums.

use thiserror::Error;

/// FFI-safe error type
#[derive(Debug, Error, uniffi::Error)]
pub enum FfiError {
    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),

    #[error("Backend error: {0}")]
    BackendError(String),

    #[error("Session error: {0}")]
    SessionError(String),

    #[error("IO error: {0}")]
    IoError(String),

    #[error("Database error: {0}")]
    DatabaseError(String),
}

impl From<anyhow::Error> for FfiError {
    fn from(e: anyhow::Error) -> Self {
        FfiError::BackendError(e.to_string())
    }
}

impl From<serde_json::Error> for FfiError {
    fn from(e: serde_json::Error) -> Self {
        FfiError::InvalidConfig(e.to_string())
    }
}
