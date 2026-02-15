// ABOUTME: Coven gateway provider for registering workspaces as agents
// ABOUTME: Manages gRPC streams to coven-gateway with heartbeat and message handling

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tonic::transport::Channel;
use uuid::Uuid;

use crate::config::CovenConfig;

/// Generated protobuf types from coven.proto
pub mod proto {
    tonic::include_proto!("coven");
}

use proto::agent_message::Payload;
use proto::coven_control_client::CovenControlClient;
use proto::{AgentMessage, AgentMetadata, Heartbeat, RegisterAgent};

/// Manages connections to coven-gateway for workspace agents
pub struct CovenProvider {
    config: CovenConfig,
    client: CovenControlClient<Channel>,
    streams: Arc<Mutex<HashMap<String, AgentStreamHandle>>>,
    workspace_dir: String,
}

/// Handle for a single agent stream with cancellation
struct AgentStreamHandle {
    _agent_id: String,
    _workspace_name: String,
    cancel: tokio::sync::watch::Sender<bool>,
}

impl CovenProvider {
    /// Create a new CovenProvider and connect to the gateway
    pub async fn new(config: CovenConfig, workspace_dir: String) -> anyhow::Result<Self> {
        let client = CovenControlClient::connect(config.gateway_addr.clone()).await?;
        tracing::info!(
            gateway = %config.gateway_addr,
            "Connected to coven gateway"
        );

        Ok(Self {
            config,
            client,
            streams: Arc::new(Mutex::new(HashMap::new())),
            workspace_dir,
        })
    }

    /// Start the provider: scan workspaces and register each as an agent
    pub async fn start(&mut self) -> anyhow::Result<()> {
        let workspaces = self.list_workspaces()?;
        tracing::info!(count = workspaces.len(), "Discovered workspaces");

        for ws_name in &workspaces {
            if let Err(e) = self.register_workspace(ws_name).await {
                tracing::error!(workspace = %ws_name, error = %e, "Failed to register workspace");
            }
        }

        // Optionally register the DISPATCH agent for control plane
        if self.config.register_dispatch {
            if let Err(e) = self.register_dispatch().await {
                tracing::error!(error = %e, "Failed to register DISPATCH agent");
            }
        }

        Ok(())
    }

    /// Register a workspace as an agent with the gateway
    async fn register_workspace(&mut self, workspace_name: &str) -> anyhow::Result<()> {
        let agent_id = self.deterministic_agent_id(workspace_name);
        let display_name = format!(
            "{}-{}",
            self.config.agent_name_prefix, workspace_name
        );

        let register = RegisterAgent {
            agent_id: agent_id.clone(),
            name: display_name,
            capabilities: vec![
                "chat".to_string(),
                "code".to_string(),
                "search".to_string(),
            ],
            metadata: Some(AgentMetadata {
                working_directory: Path::new(&self.workspace_dir)
                    .join(workspace_name)
                    .to_string_lossy()
                    .to_string(),
                git: None,
                hostname: hostname(),
                os: std::env::consts::OS.to_string(),
                workspaces: vec![workspace_name.to_string()],
                backend: "mux".to_string(),
            }),
            protocol_features: vec![
                "token_usage".to_string(),
                "cancellation".to_string(),
            ],
        };

        self.spawn_agent_stream(agent_id, workspace_name.to_string(), register)
            .await
    }

    /// Register the DISPATCH agent for control plane operations
    async fn register_dispatch(&mut self) -> anyhow::Result<()> {
        let agent_id = self.deterministic_agent_id("DISPATCH");
        let display_name = format!("{}-DISPATCH", self.config.agent_name_prefix);

        let register = RegisterAgent {
            agent_id: agent_id.clone(),
            name: display_name,
            capabilities: vec![
                "dispatch".to_string(),
                "admin".to_string(),
            ],
            metadata: Some(AgentMetadata {
                working_directory: self.workspace_dir.clone(),
                git: None,
                hostname: hostname(),
                os: std::env::consts::OS.to_string(),
                workspaces: vec![],
                backend: "dispatch".to_string(),
            }),
            protocol_features: vec![],
        };

        self.spawn_agent_stream(agent_id, "DISPATCH".to_string(), register)
            .await
    }

    /// Spawn a bidirectional gRPC stream for an agent
    async fn spawn_agent_stream(
        &mut self,
        agent_id: String,
        workspace_name: String,
        register: RegisterAgent,
    ) -> anyhow::Result<()> {
        let (cancel_tx, cancel_rx) = tokio::sync::watch::channel(false);
        let mut client = self.client.clone();
        let streams = self.streams.clone();
        let agent_id_clone = agent_id.clone();
        let ws_name_clone = workspace_name.clone();

        // Create the request stream
        let (tx, rx) = tokio::sync::mpsc::channel::<AgentMessage>(32);

        // Send registration message
        tx.send(AgentMessage {
            payload: Some(Payload::Register(register)),
        })
        .await?;

        // Convert mpsc to tonic streaming
        let request_stream = tokio_stream::wrappers::ReceiverStream::new(rx);

        // Start the bidirectional stream
        let response = client.agent_stream(request_stream).await?;
        let mut inbound = response.into_inner();

        tracing::info!(
            agent_id = %agent_id,
            workspace = %workspace_name,
            "Agent stream established"
        );

        // Spawn stream handler task
        let heartbeat_tx = tx.clone();
        tokio::spawn(async move {
            let mut cancel_rx = cancel_rx;
            let mut heartbeat_interval = tokio::time::interval(Duration::from_secs(30));

            loop {
                tokio::select! {
                    // Check for cancellation
                    _ = cancel_rx.changed() => {
                        if *cancel_rx.borrow() {
                            tracing::info!(agent_id = %agent_id_clone, "Agent stream shutting down");
                            break;
                        }
                    }
                    // Send heartbeat
                    _ = heartbeat_interval.tick() => {
                        let hb = AgentMessage {
                            payload: Some(Payload::Heartbeat(Heartbeat {
                                timestamp_ms: chrono::Utc::now().timestamp_millis(),
                            })),
                        };
                        if heartbeat_tx.send(hb).await.is_err() {
                            tracing::warn!(agent_id = %agent_id_clone, "Heartbeat send failed, stream closed");
                            break;
                        }
                    }
                    // Handle incoming server messages
                    msg = inbound.message() => {
                        match msg {
                            Ok(Some(server_msg)) => {
                                handle_server_message(&agent_id_clone, &ws_name_clone, server_msg, &tx).await;
                            }
                            Ok(None) => {
                                tracing::info!(agent_id = %agent_id_clone, "Server closed stream");
                                break;
                            }
                            Err(e) => {
                                tracing::error!(agent_id = %agent_id_clone, error = %e, "Stream error");
                                break;
                            }
                        }
                    }
                }
            }

            // Clean up stream handle
            let mut streams = streams.lock().await;
            streams.remove(&agent_id_clone);
        });

        // Store the stream handle
        let mut streams = self.streams.lock().await;
        streams.insert(
            agent_id.clone(),
            AgentStreamHandle {
                _agent_id: agent_id,
                _workspace_name: workspace_name,
                cancel: cancel_tx,
            },
        );

        Ok(())
    }

    /// Generate a deterministic agent ID from workspace name
    fn deterministic_agent_id(&self, workspace_name: &str) -> String {
        let input = format!(
            "{}:{}:{}",
            self.config.agent_name_prefix, workspace_name, self.config.gateway_addr
        );
        let uuid = Uuid::new_v5(&Uuid::NAMESPACE_DNS, input.as_bytes());
        uuid.to_string()
    }

    /// List workspace directory names
    fn list_workspaces(&self) -> anyhow::Result<Vec<String>> {
        let path = Path::new(&self.workspace_dir);
        if !path.exists() {
            return Ok(Vec::new());
        }

        let mut names: Vec<String> = std::fs::read_dir(path)?
            .filter_map(|entry| {
                let entry = entry.ok()?;
                if entry.file_type().ok()?.is_dir() {
                    let name = entry.file_name().to_string_lossy().to_string();
                    if !name.starts_with('.') {
                        Some(name)
                    } else {
                        None
                    }
                } else {
                    None
                }
            })
            .collect();
        names.sort();
        Ok(names)
    }

    /// Gracefully shut down all agent streams
    pub async fn shutdown(&self) {
        let streams = self.streams.lock().await;
        tracing::info!(count = streams.len(), "Shutting down coven agent streams");
        for (_, handle) in streams.iter() {
            let _ = handle.cancel.send(true);
        }
    }

    /// Get the number of active agent streams
    pub async fn active_streams(&self) -> usize {
        self.streams.lock().await.len()
    }
}

/// Handle an incoming server message
async fn handle_server_message(
    agent_id: &str,
    workspace: &str,
    msg: proto::ServerMessage,
    _tx: &tokio::sync::mpsc::Sender<AgentMessage>,
) {
    use proto::server_message::Payload as SP;

    match msg.payload {
        Some(SP::Welcome(welcome)) => {
            tracing::info!(
                agent_id = %agent_id,
                workspace = %workspace,
                instance_id = %welcome.instance_id,
                "Registered with coven gateway"
            );
        }
        Some(SP::SendMessage(send_msg)) => {
            tracing::info!(
                agent_id = %agent_id,
                workspace = %workspace,
                request_id = %send_msg.request_id,
                sender = %send_msg.sender,
                "Received message from gateway"
            );
            // Message handling will be implemented when AgentHandle integration is added
        }
        Some(SP::Shutdown(shutdown)) => {
            tracing::info!(
                agent_id = %agent_id,
                reason = %shutdown.reason,
                "Gateway requested shutdown"
            );
        }
        Some(SP::RegistrationError(err)) => {
            tracing::error!(
                agent_id = %agent_id,
                reason = %err.reason,
                suggested_id = %err.suggested_id,
                "Registration rejected by gateway"
            );
        }
        Some(SP::CancelRequest(cancel)) => {
            tracing::info!(
                agent_id = %agent_id,
                request_id = %cancel.request_id,
                "Cancel request from gateway"
            );
        }
        Some(SP::InjectContext(inject)) => {
            tracing::debug!(
                agent_id = %agent_id,
                injection_id = %inject.injection_id,
                "Context injection from gateway"
            );
        }
        Some(SP::ToolApproval(_)) | Some(SP::PackToolResult(_)) | None => {}
    }
}

/// Get the system hostname
fn hostname() -> String {
    gethostname::gethostname()
        .to_string_lossy()
        .to_string()
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deterministic_agent_id() {
        let config = CovenConfig {
            gateway_addr: "http://localhost:9090".to_string(),
            register_dispatch: false,
            agent_name_prefix: "gorp".to_string(),
            ssh_key_path: None,
        };
        let provider_config = config.clone();

        // Create two IDs for same workspace — should be identical
        let input1 = format!(
            "{}:{}:{}",
            provider_config.agent_name_prefix, "research", provider_config.gateway_addr
        );
        let id1 = Uuid::new_v5(&Uuid::NAMESPACE_DNS, input1.as_bytes()).to_string();

        let input2 = format!(
            "{}:{}:{}",
            config.agent_name_prefix, "research", config.gateway_addr
        );
        let id2 = Uuid::new_v5(&Uuid::NAMESPACE_DNS, input2.as_bytes()).to_string();

        assert_eq!(id1, id2);

        // Different workspace should produce different ID
        let input3 = format!(
            "{}:{}:{}",
            config.agent_name_prefix, "dev", config.gateway_addr
        );
        let id3 = Uuid::new_v5(&Uuid::NAMESPACE_DNS, input3.as_bytes()).to_string();
        assert_ne!(id1, id3);
    }

    #[test]
    fn test_deterministic_agent_id_is_valid_uuid() {
        let input = format!("{}:{}:{}", "gorp", "test", "http://localhost:9090");
        let uuid = Uuid::new_v5(&Uuid::NAMESPACE_DNS, input.as_bytes());
        assert_eq!(uuid.get_version(), Some(uuid::Version::Sha1));
    }
}
