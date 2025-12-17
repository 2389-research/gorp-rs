// ABOUTME: This module provides an ACP (Agent Client Protocol) client for communicating with AI agents.
// ABOUTME: It replaces direct Claude CLI spawning with the standardized ACP protocol over stdio.

use agent_client_protocol as acp;
use anyhow::Result;
use std::path::Path;
use tokio::sync::mpsc;

/// Events emitted during ACP agent execution
#[derive(Debug, Clone)]
pub enum AcpEvent {
    /// Agent is calling a tool
    ToolUse { name: String, input_preview: String },
    /// Text chunk from agent
    Text(String),
    /// Final result with optional usage stats
    Result { text: String },
    /// Error occurred
    Error(String),
    /// Session is invalid/orphaned
    InvalidSession,
}

/// ACP client for communicating with an agent process
pub struct AcpClient {
    // TODO: implement
}

impl AcpClient {
    /// Spawn a new agent process and establish ACP connection
    pub async fn spawn(_working_dir: &Path, _agent_binary: &str) -> Result<Self> {
        todo!("implement spawn")
    }

    /// Initialize the ACP connection
    pub async fn initialize(&self) -> Result<()> {
        todo!("implement initialize")
    }

    /// Create a new session
    pub async fn new_session(&self) -> Result<String> {
        todo!("implement new_session")
    }

    /// Load an existing session by ID
    pub async fn load_session(&self, _session_id: &str) -> Result<()> {
        todo!("implement load_session")
    }

    /// Send a prompt and receive streaming events
    pub async fn prompt(&self, _session_id: &str, _text: &str) -> Result<mpsc::Receiver<AcpEvent>> {
        todo!("implement prompt")
    }

    /// Cancel the current operation
    pub async fn cancel(&self) -> Result<()> {
        todo!("implement cancel")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_acp_event_variants() {
        let event = AcpEvent::ToolUse {
            name: "Read".to_string(),
            input_preview: "file.txt".to_string(),
        };
        assert!(matches!(event, AcpEvent::ToolUse { .. }));

        let event = AcpEvent::Result { text: "done".to_string() };
        assert!(matches!(event, AcpEvent::Result { .. }));
    }
}
