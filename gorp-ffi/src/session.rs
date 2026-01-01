// ABOUTME: FFI wrapper for SessionStore.
// ABOUTME: Provides SQLite-backed session/channel persistence.

use crate::error::FfiError;
use gorp_core::session::{Channel, SessionStore};
use std::sync::Arc;

/// FFI-safe channel record
#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiChannel {
    pub channel_name: String,
    pub room_id: String,
    pub session_id: String,
    pub directory: String,
    pub started: bool,
    pub created_at: String,
    pub backend_type: Option<String>,
}

impl From<Channel> for FfiChannel {
    fn from(c: Channel) -> Self {
        Self {
            channel_name: c.channel_name,
            room_id: c.room_id,
            session_id: c.session_id,
            directory: c.directory,
            started: c.started,
            created_at: c.created_at,
            backend_type: c.backend_type,
        }
    }
}

/// FFI wrapper for SessionStore
#[derive(uniffi::Object)]
pub struct FfiSessionStore {
    inner: SessionStore,
}

#[uniffi::export]
impl FfiSessionStore {
    /// Create/open a session store at the given workspace path
    #[uniffi::constructor]
    pub fn new(workspace_path: String) -> Result<Arc<Self>, FfiError> {
        let store = SessionStore::new(&workspace_path)
            .map_err(|e| FfiError::DatabaseError(e.to_string()))?;
        Ok(Arc::new(Self { inner: store }))
    }

    /// Create a new channel
    pub fn create_channel(
        &self,
        channel_name: String,
        room_id: String,
    ) -> Result<FfiChannel, FfiError> {
        let channel = self
            .inner
            .create_channel(&channel_name, &room_id)
            .map_err(|e| FfiError::DatabaseError(e.to_string()))?;
        Ok(channel.into())
    }

    /// Get channel by name
    pub fn get_by_name(&self, channel_name: String) -> Result<Option<FfiChannel>, FfiError> {
        let channel = self
            .inner
            .get_by_name(&channel_name)
            .map_err(|e| FfiError::DatabaseError(e.to_string()))?;
        Ok(channel.map(Into::into))
    }

    /// Get channel by room ID
    pub fn get_by_room(&self, room_id: String) -> Result<Option<FfiChannel>, FfiError> {
        let channel = self
            .inner
            .get_by_room(&room_id)
            .map_err(|e| FfiError::DatabaseError(e.to_string()))?;
        Ok(channel.map(Into::into))
    }

    /// Get channel by session ID
    pub fn get_by_session_id(&self, session_id: String) -> Result<Option<FfiChannel>, FfiError> {
        let channel = self
            .inner
            .get_by_session_id(&session_id)
            .map_err(|e| FfiError::DatabaseError(e.to_string()))?;
        Ok(channel.map(Into::into))
    }

    /// List all channels
    pub fn list_all(&self) -> Result<Vec<FfiChannel>, FfiError> {
        let channels = self
            .inner
            .list_all()
            .map_err(|e| FfiError::DatabaseError(e.to_string()))?;
        Ok(channels.into_iter().map(Into::into).collect())
    }

    /// Mark a channel as started
    pub fn mark_started(&self, room_id: String) -> Result<(), FfiError> {
        self.inner
            .mark_started(&room_id)
            .map_err(|e| FfiError::DatabaseError(e.to_string()))
    }

    /// Delete a channel by name
    pub fn delete_channel(&self, channel_name: String) -> Result<(), FfiError> {
        self.inner
            .delete_channel(&channel_name)
            .map_err(|e| FfiError::DatabaseError(e.to_string()))
    }

    /// Delete a channel by room ID
    pub fn delete_by_room(&self, room_id: String) -> Result<Option<String>, FfiError> {
        self.inner
            .delete_by_room(&room_id)
            .map_err(|e| FfiError::DatabaseError(e.to_string()))
    }

    /// Reset a channel's session
    pub fn reset_session(
        &self,
        channel_name: String,
        new_session_id: String,
    ) -> Result<(), FfiError> {
        self.inner
            .reset_session(&channel_name, &new_session_id)
            .map_err(|e| FfiError::DatabaseError(e.to_string()))
    }

    /// Update backend type for a channel
    pub fn update_backend_type(
        &self,
        channel_name: String,
        backend_type: Option<String>,
    ) -> Result<(), FfiError> {
        self.inner
            .update_backend_type(&channel_name, backend_type.as_deref())
            .map_err(|e| FfiError::DatabaseError(e.to_string()))
    }

    /// Get a setting value
    pub fn get_setting(&self, key: String) -> Result<Option<String>, FfiError> {
        self.inner
            .get_setting(&key)
            .map_err(|e| FfiError::DatabaseError(e.to_string()))
    }

    /// Set a setting value
    pub fn set_setting(&self, key: String, value: String) -> Result<(), FfiError> {
        self.inner
            .set_setting(&key, &value)
            .map_err(|e| FfiError::DatabaseError(e.to_string()))
    }
}

impl FfiSessionStore {
    /// Get the inner SessionStore (for SchedulerStore creation)
    pub(crate) fn inner(&self) -> &SessionStore {
        &self.inner
    }
}
