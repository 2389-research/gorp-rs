// ABOUTME: Manages warm Claude Code sessions to avoid 2-minute startup latency.
// ABOUTME: Keeps AcpClient instances alive per channel, with lazy creation and TTL cleanup.

use crate::acp_client::AcpClient;
use std::collections::HashMap;
use std::time::{Duration, Instant};
#[cfg(test)]
use tokio::sync::mpsc;

/// Configuration for warm session behavior
#[derive(Debug, Clone)]
pub struct WarmConfig {
    pub keep_alive_duration: Duration,
    pub pre_warm_lead_time: Duration,
    pub agent_binary: String,
}

/// A warm session holding an active AcpClient
struct WarmSession {
    client: AcpClient,
    session_id: String,
    last_used: Instant,
    channel_name: String,
}

/// Manages warm Claude Code sessions across channels
pub struct WarmSessionManager {
    sessions: HashMap<String, WarmSession>,
    config: WarmConfig,
}

impl WarmSessionManager {
    pub fn new(config: WarmConfig) -> Self {
        Self {
            sessions: HashMap::new(),
            config,
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
    pub fn cleanup_stale(&mut self) {
        let now = Instant::now();
        let keep_alive = self.config.keep_alive_duration;

        self.sessions.retain(|channel_name, session| {
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
        });
    }

    #[cfg(test)]
    fn inject_test_session(
        &mut self,
        channel_name: String,
        session_id: String,
        last_used: Instant,
    ) {
        // Create a mock AcpClient with a dummy mpsc channel
        let (tx, _rx) = mpsc::channel(1);
        let client = AcpClient::new_test_mock_blocking(tx);

        let session = WarmSession {
            client,
            session_id,
            last_used,
            channel_name: channel_name.clone(),
        };

        self.sessions.insert(channel_name, session);
    }
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

    #[test]
    fn test_cleanup_stale_removes_old_sessions() {
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

    #[test]
    fn test_cleanup_stale_keeps_all_recent_sessions() {
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
