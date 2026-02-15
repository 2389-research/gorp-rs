// ABOUTME: Coven gateway provider for registering workspaces as agents
// ABOUTME: Manages gRPC streams to coven-gateway with heartbeat and message handling

pub mod reconnect;
pub mod stream;

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tonic::transport::Channel;
use uuid::Uuid;

use crate::config::CovenConfig;
use crate::session::SessionStore;
use gorp_agent::AgentHandle;
use gorp_core::warm_session::SharedWarmSessionManager;

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
    warm_manager: SharedWarmSessionManager,
    session_store: Arc<SessionStore>,
}

/// Handle for a single agent stream with cancellation
struct AgentStreamHandle {
    _agent_id: String,
    _workspace_name: String,
    cancel: tokio::sync::watch::Sender<bool>,
}

impl CovenProvider {
    /// Create a new CovenProvider and connect to the gateway
    pub async fn new(
        config: CovenConfig,
        workspace_dir: String,
        warm_manager: SharedWarmSessionManager,
        session_store: Arc<SessionStore>,
    ) -> anyhow::Result<Self> {
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
            warm_manager,
            session_store,
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

    /// Create an AgentHandle for a workspace using the warm session registry
    fn create_workspace_handle(&self, workspace_name: &str) -> anyhow::Result<AgentHandle> {
        let working_dir = Path::new(&self.workspace_dir)
            .join(workspace_name)
            .to_string_lossy()
            .to_string();

        // Use the warm session manager's registry to create the handle
        // This is synchronous and returns immediately — the backend worker starts in background
        // block_in_place is required because this sync fn is called from async context
        let mgr = tokio::task::block_in_place(|| self.warm_manager.blocking_read());
        let warm_config = mgr.config();
        let registry = mgr.registry();
        drop(mgr);

        gorp_core::warm_session::WarmSessionManager::create_agent_handle_with_config(
            &registry,
            &working_dir,
            &warm_config,
            None,
        )
    }

    /// Create an AgentHandle for DISPATCH with dispatch-specific tools
    fn create_dispatch_handle(&self) -> anyhow::Result<AgentHandle> {
        use gorp_agent::backends::mux::{MuxBackend, MuxConfig};
        use std::path::PathBuf;

        // block_in_place is required because this sync fn is called from async context
        let mgr = tokio::task::block_in_place(|| self.warm_manager.blocking_read());
        let warm_config = mgr.config();
        drop(mgr);

        let model = warm_config.model.clone().ok_or_else(|| {
            anyhow::anyhow!("No model configured for DISPATCH. Set 'model' in config.toml under [mux] section.")
        })?;

        let dispatch_working_dir = std::env::temp_dir().join("gorp-dispatch");
        if let Err(e) = std::fs::create_dir_all(&dispatch_working_dir) {
            tracing::warn!(error = %e, "Failed to create DISPATCH working directory");
        }

        let mux_config = MuxConfig {
            model,
            max_tokens: warm_config.max_tokens.unwrap_or(8192),
            working_dir: dispatch_working_dir,
            global_system_prompt_path: warm_config
                .global_system_prompt_path
                .clone()
                .map(PathBuf::from),
            local_prompt_files: vec![],
            mcp_servers: vec![],
        };

        let dispatch_tools =
            crate::dispatch_tools::create_dispatch_tools(Arc::clone(&self.session_store));

        let agent_handle = MuxBackend::new(mux_config)?
            .with_tools(dispatch_tools)
            .into_handle();

        Ok(agent_handle)
    }

    /// Spawn a bidirectional gRPC stream for an agent with automatic reconnection
    async fn spawn_agent_stream(
        &mut self,
        agent_id: String,
        workspace_name: String,
        register: RegisterAgent,
    ) -> anyhow::Result<()> {
        let (cancel_tx, cancel_rx) = tokio::sync::watch::channel(false);
        let client = self.client.clone();
        let streams = self.streams.clone();

        // Create the AgentHandle for this stream
        let is_dispatch = workspace_name == "DISPATCH";
        let agent_handle = if is_dispatch {
            self.create_dispatch_handle()?
        } else {
            self.create_workspace_handle(&workspace_name)?
        };

        // Clone session_store for DISPATCH routing
        let session_store = Arc::clone(&self.session_store);

        // Make the initial connection (fail fast if gateway is unreachable)
        let (tx, inbound) = connect_stream(&mut client.clone(), &register).await?;

        tracing::info!(
            agent_id = %agent_id,
            workspace = %workspace_name,
            "Agent stream established"
        );

        // Spawn reconnecting stream handler task
        let agent_id_clone = agent_id.clone();
        let ws_name_clone = workspace_name.clone();
        tokio::spawn(async move {
            let mut cancel_rx = cancel_rx;
            let mut sessions: HashMap<String, String> = HashMap::new();
            let mut backoff = reconnect::BackoffState::new(reconnect::BackoffConfig::default());
            let mut client = client;
            let mut tx = tx;
            let mut inbound = inbound;

            'reconnect: loop {
                // Stream connected — reset backoff
                backoff.record_success();

                // Run the message loop until stream drops or shutdown
                let should_shutdown = run_stream_loop(
                    &agent_id_clone,
                    &ws_name_clone,
                    is_dispatch,
                    &mut cancel_rx,
                    &mut inbound,
                    &agent_handle,
                    &mut sessions,
                    &session_store,
                    &tx,
                )
                .await;

                if should_shutdown {
                    break;
                }

                // Stream dropped — attempt reconnection with backoff
                loop {
                    // Check for cancellation before retrying
                    if *cancel_rx.borrow() {
                        break 'reconnect;
                    }

                    match backoff.record_failure() {
                        Some(delay) => {
                            tracing::info!(
                                agent_id = %agent_id_clone,
                                delay_secs = delay.as_secs(),
                                attempt = backoff.consecutive_failures(),
                                "Reconnecting after backoff"
                            );

                            // Wait for backoff delay, but allow cancellation to interrupt
                            tokio::select! {
                                _ = tokio::time::sleep(delay) => {}
                                _ = cancel_rx.changed() => {
                                    if *cancel_rx.borrow() {
                                        break 'reconnect;
                                    }
                                }
                            }
                        }
                        None => {
                            tracing::error!(
                                agent_id = %agent_id_clone,
                                "Max reconnection retries exceeded, giving up"
                            );
                            break 'reconnect;
                        }
                    }

                    // Attempt reconnection
                    match connect_stream(&mut client, &register).await {
                        Ok((new_tx, new_inbound)) => {
                            tracing::info!(
                                agent_id = %agent_id_clone,
                                workspace = %ws_name_clone,
                                "Reconnected to coven gateway"
                            );
                            tx = new_tx;
                            inbound = new_inbound;
                            continue 'reconnect;
                        }
                        Err(e) => {
                            tracing::error!(
                                agent_id = %agent_id_clone,
                                error = %e,
                                "Reconnection attempt failed"
                            );
                            // Continue the inner retry loop
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

/// Establish a gRPC stream connection and send the registration message
async fn connect_stream(
    client: &mut CovenControlClient<Channel>,
    register: &RegisterAgent,
) -> anyhow::Result<(
    tokio::sync::mpsc::Sender<AgentMessage>,
    tonic::Streaming<proto::ServerMessage>,
)> {
    let (tx, rx) = tokio::sync::mpsc::channel::<AgentMessage>(32);

    // Send registration message
    tx.send(AgentMessage {
        payload: Some(Payload::Register(register.clone())),
    })
    .await?;

    // Convert mpsc to tonic streaming
    let request_stream = tokio_stream::wrappers::ReceiverStream::new(rx);

    // Start the bidirectional stream
    let response = client.agent_stream(request_stream).await?;
    let inbound = response.into_inner();

    Ok((tx, inbound))
}

/// Run the message loop for an agent stream.
/// Returns `true` if shutdown was requested (should NOT reconnect),
/// `false` if the stream dropped (SHOULD reconnect).
async fn run_stream_loop(
    agent_id: &str,
    workspace: &str,
    is_dispatch: bool,
    cancel_rx: &mut tokio::sync::watch::Receiver<bool>,
    inbound: &mut tonic::Streaming<proto::ServerMessage>,
    agent_handle: &AgentHandle,
    sessions: &mut HashMap<String, String>,
    session_store: &SessionStore,
    tx: &tokio::sync::mpsc::Sender<AgentMessage>,
) -> bool {
    let mut heartbeat_interval = tokio::time::interval(Duration::from_secs(30));

    loop {
        tokio::select! {
            // Check for cancellation
            _ = cancel_rx.changed() => {
                if *cancel_rx.borrow() {
                    tracing::info!(agent_id = %agent_id, "Agent stream shutting down");
                    return true; // Shutdown — don't reconnect
                }
            }
            // Send heartbeat
            _ = heartbeat_interval.tick() => {
                let hb = AgentMessage {
                    payload: Some(Payload::Heartbeat(Heartbeat {
                        timestamp_ms: chrono::Utc::now().timestamp_millis(),
                    })),
                };
                if tx.send(hb).await.is_err() {
                    tracing::warn!(agent_id = %agent_id, "Heartbeat send failed, stream closed");
                    return false; // Stream dropped — reconnect
                }
            }
            // Handle incoming server messages
            msg = inbound.message() => {
                match msg {
                    Ok(Some(server_msg)) => {
                        let should_shutdown = handle_server_message(
                            agent_id,
                            workspace,
                            is_dispatch,
                            server_msg,
                            agent_handle,
                            sessions,
                            session_store,
                            tx,
                        ).await;
                        if should_shutdown {
                            return true; // Gateway requested shutdown — don't reconnect
                        }
                    }
                    Ok(None) => {
                        tracing::info!(agent_id = %agent_id, "Server closed stream");
                        return false; // Stream dropped — reconnect
                    }
                    Err(e) => {
                        tracing::error!(agent_id = %agent_id, error = %e, "Stream error");
                        return false; // Stream errored — reconnect
                    }
                }
            }
        }
    }
}

/// Handle an incoming server message by routing to the appropriate handler.
/// Returns true if the stream should be shut down (don't reconnect).
async fn handle_server_message(
    agent_id: &str,
    workspace: &str,
    is_dispatch: bool,
    msg: proto::ServerMessage,
    agent_handle: &AgentHandle,
    sessions: &mut HashMap<String, String>,
    session_store: &SessionStore,
    tx: &tokio::sync::mpsc::Sender<AgentMessage>,
) -> bool {
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

            let result = if is_dispatch {
                stream::handle_dispatch_message(
                    &send_msg,
                    agent_handle,
                    sessions,
                    session_store,
                    tx,
                )
                .await
            } else {
                stream::handle_send_message(&send_msg, agent_handle, sessions, tx).await
            };

            if let Err(e) = result {
                tracing::error!(
                    agent_id = %agent_id,
                    workspace = %workspace,
                    request_id = %send_msg.request_id,
                    error = %e,
                    "Failed to handle message"
                );
                // Send error response back to gateway
                let error_msg = AgentMessage {
                    payload: Some(Payload::Response(proto::MessageResponse {
                        request_id: send_msg.request_id,
                        event: Some(proto::message_response::Event::Error(e.to_string())),
                    })),
                };
                let _ = tx.send(error_msg).await;
            }
        }
        Some(SP::Shutdown(shutdown)) => {
            tracing::info!(
                agent_id = %agent_id,
                reason = %shutdown.reason,
                "Gateway requested shutdown — stopping stream"
            );
            return true;
        }
        Some(SP::RegistrationError(err)) => {
            tracing::error!(
                agent_id = %agent_id,
                reason = %err.reason,
                suggested_id = %err.suggested_id,
                "Registration rejected by gateway — stopping stream"
            );
            return true;
        }
        Some(SP::CancelRequest(cancel)) => {
            tracing::info!(
                agent_id = %agent_id,
                request_id = %cancel.request_id,
                "Cancel request from gateway"
            );
            if let Err(e) =
                stream::handle_cancel_request(&cancel, agent_handle, sessions, tx).await
            {
                tracing::error!(
                    agent_id = %agent_id,
                    error = %e,
                    "Failed to handle cancel request"
                );
            }
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

    false
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
