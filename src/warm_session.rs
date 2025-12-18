// ABOUTME: Manages warm Claude Code sessions to avoid 2-minute startup latency.
// ABOUTME: Keeps AcpClient instances alive per channel, with lazy creation and TTL cleanup.

use crate::acp_client::{AcpClient, AcpEvent};
use crate::session::Channel;
use anyhow::Result;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, RwLock};

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
}
