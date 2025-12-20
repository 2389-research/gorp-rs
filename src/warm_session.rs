// ABOUTME: Manages warm Claude Code sessions to avoid 2-minute startup latency.
// ABOUTME: Keeps AgentHandle instances alive per channel, with lazy creation and TTL cleanup.

use crate::session::Channel;
use anyhow::Result;
use gorp_agent::{AgentEvent, AgentHandle, AgentRegistry};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, Mutex, RwLock};

/// Configuration for warm session behavior
#[derive(Debug, Clone)]
pub struct WarmConfig {
    pub keep_alive_duration: Duration,
    pub pre_warm_lead_time: Duration,
    pub agent_binary: String,
}

/// A warm session holding an active AgentHandle
/// Each session is wrapped in its own Mutex for per-channel locking
/// Fields are private to ensure proper locking semantics
pub struct WarmSession {
    handle: AgentHandle,
    session_id: String,
    last_used: Instant,
    /// Channel to send events to - updated before each prompt
    event_tx: Option<mpsc::Sender<AgentEvent>>,
}

/// Handle to a warm session, allowing concurrent access across channels
pub type WarmSessionHandle = Arc<Mutex<WarmSession>>;

/// Manages warm Claude Code sessions across channels
/// Uses per-channel locking to allow concurrent prompts across different channels
pub struct WarmSessionManager {
    /// Map of channel names to their session handles
    /// Each session has its own Mutex for per-channel locking
    sessions: HashMap<String, WarmSessionHandle>,
    config: WarmConfig,
    /// Registry for creating agent backends
    registry: AgentRegistry,
}

impl WarmSessionManager {
    pub fn new(config: WarmConfig) -> Self {
        Self {
            sessions: HashMap::new(),
            config,
            registry: AgentRegistry::default(),
        }
    }

    /// Create with a custom registry (useful for testing)
    pub fn with_registry(config: WarmConfig, registry: AgentRegistry) -> Self {
        Self {
            sessions: HashMap::new(),
            config,
            registry,
        }
    }

    /// Get the agent binary path
    pub fn agent_binary(&self) -> &str {
        &self.config.agent_binary
    }

    /// Get the keep-alive duration
    pub fn keep_alive_duration(&self) -> Duration {
        self.config.keep_alive_duration
    }

    /// Remove sessions that have been idle longer than keep_alive_duration
    /// Note: This requires a write lock on the manager
    pub fn cleanup_stale(&mut self) {
        let now = Instant::now();
        let keep_alive = self.config.keep_alive_duration;

        self.sessions.retain(|channel_name, handle| {
            // Try to lock the session - if locked, it's in use and not stale
            match handle.try_lock() {
                Ok(session) => {
                    let age = now.duration_since(session.last_used);
                    if age > keep_alive {
                        tracing::info!(
                            channel = %channel_name,
                            idle_secs = age.as_secs(),
                            "Removing stale warm session"
                        );
                        false
                    } else {
                        true
                    }
                }
                Err(_) => {
                    // Session is locked (in use), keep it
                    true
                }
            }
        });
    }

    /// Create an AgentHandle using the registry
    fn create_agent_handle(&self, working_dir: &str) -> Result<AgentHandle> {
        // Use ACP backend by default (matches previous behavior)
        let config = serde_json::json!({
            "working_dir": working_dir,
            "agent_binary": self.config.agent_binary,
        });

        self.registry.create("acp", &config)
    }

    /// Get an existing session handle or create a new one
    /// Returns (session_handle, session_id, is_new_session)
    pub async fn get_or_create_session(
        &mut self,
        channel: &Channel,
    ) -> Result<(WarmSessionHandle, String, bool)> {
        let channel_name = &channel.channel_name;

        // Check if we have a warm session in memory
        if let Some(handle) = self.sessions.get(channel_name) {
            let mut session = handle.lock().await;
            session.last_used = Instant::now();
            let session_id = session.session_id.clone();
            tracing::info!(channel = %channel_name, session_id = %session_id, "Reusing warm session");
            return Ok((Arc::clone(handle), session_id, false));
        }

        // Need to create a new agent handle
        tracing::info!(channel = %channel_name, "Creating new agent handle");

        // Use absolute path for working directory
        let working_dir = std::path::Path::new(&channel.directory)
            .canonicalize()
            .unwrap_or_else(|_| std::path::Path::new(&channel.directory).to_path_buf());
        let working_dir_str = working_dir.to_string_lossy().to_string();

        tracing::info!(channel = %channel_name, working_dir = %working_dir_str, "Using working directory");

        let agent_handle = self.create_agent_handle(&working_dir_str)?;

        // Try to resume existing session if channel has one
        let (session_id, is_new) = if channel.started && !channel.session_id.is_empty() {
            tracing::info!(channel = %channel_name, session_id = %channel.session_id, "Attempting to resume existing session");
            match agent_handle.load_session(&channel.session_id).await {
                Ok(()) => {
                    tracing::info!(channel = %channel_name, session_id = %channel.session_id, "Successfully resumed session");
                    (channel.session_id.clone(), false)
                }
                Err(e) => {
                    tracing::warn!(channel = %channel_name, session_id = %channel.session_id, error = %e, "Failed to resume session, creating new one");
                    let new_id = agent_handle.new_session().await?;
                    tracing::info!(channel = %channel_name, session_id = %new_id, "Created new session after resume failure");
                    (new_id, true)
                }
            }
        } else {
            // No existing session, create new one
            tracing::info!(channel = %channel_name, "Creating new session");
            let new_id = agent_handle.new_session().await?;
            tracing::info!(channel = %channel_name, session_id = %new_id, "Created new session");
            (new_id, true)
        };

        let warm_session = WarmSession {
            handle: agent_handle,
            session_id: session_id.clone(),
            last_used: Instant::now(),
            event_tx: None,
        };

        let handle = Arc::new(Mutex::new(warm_session));
        self.sessions
            .insert(channel_name.clone(), Arc::clone(&handle));

        Ok((handle, session_id, is_new))
    }

    /// Prepare a session for prompting - sets up event channel and returns handle + session_id
    /// The returned handle should be used for send_prompt WITHOUT holding the manager lock
    /// Returns (session_handle, session_id, is_new_session)
    pub async fn prepare_session(
        &mut self,
        channel: &Channel,
        event_tx: mpsc::Sender<AgentEvent>,
    ) -> Result<(WarmSessionHandle, String, bool)> {
        let channel_name = &channel.channel_name;

        // Check if we have an existing warm session
        if let Some(handle) = self.sessions.get(channel_name) {
            let mut session = handle.lock().await;
            session.last_used = Instant::now();
            session.event_tx = Some(event_tx);
            let session_id = session.session_id.clone();
            tracing::info!(channel = %channel_name, session_id = %session_id, "Prepared warm session for prompt");
            return Ok((Arc::clone(handle), session_id, false));
        }

        // Create new session (may resume existing or create fresh)
        let (handle, session_id, is_new) = self.get_or_create_session(channel).await?;

        // Set up event channel
        {
            let mut session = handle.lock().await;
            session.event_tx = Some(event_tx);
        }

        tracing::info!(channel = %channel_name, session_id = %session_id, is_new = is_new, "Prepared session for prompt");
        Ok((handle, session_id, is_new))
    }

    /// Pre-warm a session for a channel (called before scheduled prompts)
    /// Returns Some(session_id) if a NEW session was created (caller should update store)
    /// Returns None if channel was already warm or session was resumed
    pub async fn pre_warm(&mut self, channel: &Channel) -> Result<Option<String>> {
        let channel_name = &channel.channel_name;

        if self.sessions.contains_key(channel_name) {
            tracing::debug!(channel = %channel_name, "Channel already warm");
            return Ok(None);
        }

        tracing::info!(channel = %channel_name, "Pre-warming channel");
        let (_handle, session_id, is_new) = self.get_or_create_session(channel).await?;

        if is_new {
            Ok(Some(session_id))
        } else {
            Ok(None)
        }
    }

    #[cfg(test)]
    fn inject_test_session(
        &mut self,
        channel_name: String,
        session_id: String,
        last_used: Instant,
    ) {
        use gorp_agent::backends::mock::MockBackend;

        let mock = MockBackend::new();
        let handle = mock.into_handle();

        let session = WarmSession {
            handle,
            session_id,
            last_used,
            event_tx: None,
        };

        self.sessions
            .insert(channel_name, Arc::new(Mutex::new(session)));
    }
}

/// Send a prompt using a session handle - does NOT require manager lock
/// This allows concurrent prompts across different channels
pub async fn send_prompt_with_handle(
    handle: &WarmSessionHandle,
    session_id: &str,
    text: &str,
) -> Result<()> {
    tracing::info!(session_id = %session_id, prompt_len = text.len(), "send_prompt_with_handle: acquiring session lock");
    let session = handle.lock().await;
    tracing::info!(session_id = %session_id, "send_prompt_with_handle: lock acquired, calling prompt");

    // Send prompt and get event receiver
    let mut receiver = session.handle.prompt(session_id, text).await?;

    // Forward events to the session's event channel if set
    if let Some(ref event_tx) = session.event_tx {
        // Spawn a task to forward events
        let event_tx = event_tx.clone();
        tokio::spawn(async move {
            while let Some(event) = receiver.recv().await {
                if event_tx.send(event).await.is_err() {
                    break;
                }
            }
        });
    }

    tracing::info!(session_id = %session_id, "send_prompt_with_handle: prompt started");
    Ok(())
}

/// Thread-safe wrapper for WarmSessionManager
pub type SharedWarmSessionManager = Arc<RwLock<WarmSessionManager>>;

/// Create a new shared warm session manager
pub fn create_shared_manager(config: WarmConfig) -> SharedWarmSessionManager {
    Arc::new(RwLock::new(WarmSessionManager::new(config)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_warm_session_manager_creation() {
        let config = WarmConfig {
            keep_alive_duration: Duration::from_secs(3600),
            pre_warm_lead_time: Duration::from_secs(300),
            agent_binary: "claude-code-acp".to_string(),
        };
        let manager = WarmSessionManager::new(config);
        assert_eq!(manager.agent_binary(), "claude-code-acp");
        assert_eq!(manager.keep_alive_duration(), Duration::from_secs(3600));
    }

    #[tokio::test]
    async fn test_cleanup_stale_removes_old_sessions() {
        let config = WarmConfig {
            keep_alive_duration: Duration::from_secs(2), // 2 seconds for test
            pre_warm_lead_time: Duration::from_secs(300),
            agent_binary: "claude-code-acp".to_string(),
        };
        let mut manager = WarmSessionManager::new(config);

        // Insert a recent session (should NOT be cleaned up)
        let recent_time = Instant::now();
        manager.inject_test_session(
            "recent_channel".to_string(),
            "session_123".to_string(),
            recent_time,
        );

        // Insert a stale session (3 seconds old, exceeds 2 second keep-alive)
        let stale_time = Instant::now() - Duration::from_secs(3);
        manager.inject_test_session(
            "stale_channel".to_string(),
            "session_456".to_string(),
            stale_time,
        );

        // Verify both sessions exist
        assert_eq!(manager.sessions.len(), 2);

        // Run cleanup
        manager.cleanup_stale();

        // Only the recent session should remain
        assert_eq!(manager.sessions.len(), 1);
        assert!(manager.sessions.contains_key("recent_channel"));
        assert!(!manager.sessions.contains_key("stale_channel"));
    }

    #[test]
    fn test_cleanup_stale_with_no_sessions() {
        let config = WarmConfig {
            keep_alive_duration: Duration::from_secs(1),
            pre_warm_lead_time: Duration::from_secs(300),
            agent_binary: "claude-code-acp".to_string(),
        };
        let mut manager = WarmSessionManager::new(config);

        // cleanup_stale should be callable without panic on empty manager
        manager.cleanup_stale();
        assert_eq!(manager.sessions.len(), 0);
    }

    #[tokio::test]
    async fn test_cleanup_stale_keeps_all_recent_sessions() {
        let config = WarmConfig {
            keep_alive_duration: Duration::from_secs(10), // 10 seconds
            pre_warm_lead_time: Duration::from_secs(300),
            agent_binary: "claude-code-acp".to_string(),
        };
        let mut manager = WarmSessionManager::new(config);

        // Insert three recent sessions
        for i in 0..3 {
            manager.inject_test_session(
                format!("channel_{}", i),
                format!("session_{}", i),
                Instant::now(),
            );
        }

        assert_eq!(manager.sessions.len(), 3);

        // Run cleanup - none should be removed
        manager.cleanup_stale();
        assert_eq!(manager.sessions.len(), 3);
    }
}
