// ABOUTME: FFI wrappers for AgentHandle and AgentRegistry.
// ABOUTME: Main interface for creating agents and sending prompts.

use crate::error::FfiError;
use crate::events::{dispatch_event, AgentEventCallback};
use crate::runtime::{block_on, spawn};
use gorp_agent::{AgentHandle, AgentRegistry};
use std::sync::Arc;

/// FFI-safe wrapper around AgentHandle
#[derive(uniffi::Object)]
pub struct FfiAgentHandle {
    inner: AgentHandle,
}

#[uniffi::export]
impl FfiAgentHandle {
    /// Get the backend name
    pub fn name(&self) -> String {
        self.inner.name().to_string()
    }

    /// Create a new session, returns session ID
    pub fn new_session(&self) -> Result<String, FfiError> {
        block_on(self.inner.new_session()).map_err(Into::into)
    }

    /// Load an existing session by ID
    pub fn load_session(&self, session_id: String) -> Result<(), FfiError> {
        block_on(self.inner.load_session(&session_id)).map_err(Into::into)
    }

    /// Send a prompt with streaming callback
    ///
    /// Returns immediately. Events are delivered via callback on background thread.
    pub fn prompt(
        &self,
        session_id: String,
        text: String,
        callback: Box<dyn AgentEventCallback>,
    ) -> Result<(), FfiError> {
        let handle = self.inner.clone();
        // Convert to Arc for use in async context
        let callback: Arc<dyn AgentEventCallback> = Arc::from(callback);

        spawn(async move {
            match handle.prompt(&session_id, &text).await {
                Ok(mut receiver) => {
                    while let Some(event) = receiver.recv().await {
                        dispatch_event(callback.as_ref(), event);
                    }
                }
                Err(e) => {
                    callback.on_error(
                        crate::events::FfiErrorCode::Unknown,
                        e.to_string(),
                        false,
                    );
                }
            }
        });

        Ok(())
    }

    /// Cancel an in-progress prompt
    pub fn cancel(&self, session_id: String) -> Result<(), FfiError> {
        block_on(self.inner.cancel(&session_id)).map_err(Into::into)
    }

    /// Abandon a session that was created but never used
    pub fn abandon_session(&self, session_id: String) {
        self.inner.abandon_session(&session_id);
    }
}

/// FFI-safe wrapper around AgentRegistry
#[derive(uniffi::Object)]
pub struct FfiAgentRegistry {
    inner: AgentRegistry,
}

impl Default for FfiAgentRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[uniffi::export]
impl FfiAgentRegistry {
    /// Create a new registry with all available backends
    #[uniffi::constructor]
    pub fn new() -> Self {
        Self {
            inner: AgentRegistry::default(),
        }
    }

    /// List available backend names
    pub fn available_backends(&self) -> Vec<String> {
        self.inner
            .available()
            .into_iter()
            .map(String::from)
            .collect()
    }

    /// Create a backend by name with JSON configuration
    pub fn create(&self, name: String, config_json: String) -> Result<Arc<FfiAgentHandle>, FfiError> {
        let config: serde_json::Value = serde_json::from_str(&config_json)?;
        let handle = self.inner.create(&name, &config)?;
        Ok(Arc::new(FfiAgentHandle { inner: handle }))
    }
}
