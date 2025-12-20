// ABOUTME: ACP protocol backend - communicates with claude-code-acp or codex-acp.
// ABOUTME: Wraps agent-client-protocol crate, handles !Send futures via worker task.

use crate::event::{AgentEvent, ErrorCode};
use crate::handle::{AgentHandle, Command};
use agent_client_protocol as acp;
use acp::Agent as _;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::process::{Child, Command as ProcessCommand};
use tokio::sync::mpsc;
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

/// Configuration for the ACP backend
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcpConfig {
    /// Path to the ACP binary (codex-acp or claude-code-acp)
    pub binary: String,
    /// Timeout in seconds for prompts
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
    /// Working directory for the agent
    pub working_dir: PathBuf,
}

fn default_timeout() -> u64 {
    300 // 5 minutes
}

/// Handler for ACP client-side callbacks
/// Sends events directly to the provided channel for true streaming
struct AcpClientHandler {
    event_tx: Arc<std::sync::RwLock<mpsc::Sender<AgentEvent>>>,
    working_dir: PathBuf,
}

impl AcpClientHandler {
    fn new(event_tx: Arc<std::sync::RwLock<mpsc::Sender<AgentEvent>>>, working_dir: PathBuf) -> Self {
        Self {
            event_tx,
            working_dir,
        }
    }

    fn send_event(&self, event: AgentEvent) {
        let tx = self.event_tx.read().unwrap();
        if let Err(e) = tx.try_send(event) {
            match e {
                mpsc::error::TrySendError::Full(dropped_event) => {
                    tracing::warn!(
                        event = ?dropped_event,
                        "Event channel buffer full (2048), dropping event"
                    );
                }
                mpsc::error::TrySendError::Closed(_) => {
                    tracing::debug!("Event channel closed, receiver dropped");
                }
            }
        }
    }
}

#[async_trait::async_trait(?Send)]
impl acp::Client for AcpClientHandler {
    async fn request_permission(
        &self,
        args: acp::RequestPermissionRequest,
    ) -> acp::Result<acp::RequestPermissionResponse> {
        tracing::debug!(
            session_id = %args.session_id,
            tool_call_id = %args.tool_call.tool_call_id,
            "Auto-approving permission request"
        );

        // Find an "allow once" option to approve
        let allow_option = args
            .options
            .iter()
            .find(|opt| matches!(opt.kind, acp::PermissionOptionKind::AllowOnce))
            .or_else(|| args.options.first());

        if let Some(option) = allow_option {
            Ok(acp::RequestPermissionResponse::new(
                acp::RequestPermissionOutcome::Selected(acp::SelectedPermissionOutcome::new(
                    option.option_id.clone(),
                )),
            ))
        } else {
            Ok(acp::RequestPermissionResponse::new(
                acp::RequestPermissionOutcome::Cancelled,
            ))
        }
    }

    async fn session_notification(&self, args: acp::SessionNotification) -> acp::Result<()> {
        tracing::debug!(session_id = %args.session_id, "Received session notification");
        match args.update {
            acp::SessionUpdate::AgentMessageChunk(chunk) => {
                let text = match chunk.content {
                    acp::ContentBlock::Text(t) => t.text,
                    acp::ContentBlock::Image(_) => "<image>".into(),
                    acp::ContentBlock::Audio(_) => "<audio>".into(),
                    acp::ContentBlock::ResourceLink(r) => r.uri,
                    acp::ContentBlock::Resource(_) => "<resource>".into(),
                    _ => String::new(),
                };
                if !text.is_empty() {
                    self.send_event(AgentEvent::Text(text));
                }
            }
            acp::SessionUpdate::ToolCall(tool_call) => {
                let name = tool_call.title.clone();
                let id = tool_call.tool_call_id.to_string();
                let input = tool_call.raw_input.clone().unwrap_or(serde_json::json!({}));

                self.send_event(AgentEvent::ToolStart { id, name, input });
            }
            acp::SessionUpdate::AgentThoughtChunk(chunk) => {
                let text = match chunk.content {
                    acp::ContentBlock::Text(t) => t.text,
                    acp::ContentBlock::Image(_) => "<image>".into(),
                    acp::ContentBlock::Audio(_) => "<audio>".into(),
                    acp::ContentBlock::ResourceLink(r) => r.uri,
                    acp::ContentBlock::Resource(_) => "<resource>".into(),
                    _ => String::new(),
                };
                if !text.is_empty() {
                    tracing::debug!(text_len = text.len(), "Received AgentThoughtChunk");
                    self.send_event(AgentEvent::Text(text));
                }
            }
            other => {
                tracing::debug!(?other, "Ignoring unhandled session update type");
            }
        }
        Ok(())
    }

    async fn write_text_file(
        &self,
        args: acp::WriteTextFileRequest,
    ) -> acp::Result<acp::WriteTextFileResponse> {
        let path = self.working_dir.join(&args.path);

        // Security: ensure path stays within working directory
        let canonical = match path.canonicalize() {
            Ok(p) => p,
            Err(_) => {
                if let Some(parent) = path.parent() {
                    if let Ok(canonical_parent) = parent.canonicalize() {
                        if !canonical_parent.starts_with(&self.working_dir) {
                            tracing::warn!(path = %args.path.display(), "Write attempt outside working directory");
                            return Err(acp::Error::invalid_params());
                        }
                    }
                }
                path.clone()
            }
        };

        if canonical != path && !canonical.starts_with(&self.working_dir) {
            tracing::warn!(path = %args.path.display(), "Write attempt outside working directory");
            return Err(acp::Error::invalid_params());
        }

        if let Some(parent) = path.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                tracing::error!(path = %parent.display(), error = %e, "Failed to create parent directories");
                return Err(acp::Error::internal_error());
            }
        }

        if let Err(e) = std::fs::write(&path, &args.content) {
            tracing::error!(path = %path.display(), error = %e, "Failed to write file");
            return Err(acp::Error::internal_error());
        }

        tracing::debug!(path = %args.path.display(), len = args.content.len(), "Wrote file");
        Ok(acp::WriteTextFileResponse::new())
    }

    async fn read_text_file(
        &self,
        args: acp::ReadTextFileRequest,
    ) -> acp::Result<acp::ReadTextFileResponse> {
        let path = self.working_dir.join(&args.path);

        let canonical = match path.canonicalize() {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!(path = %args.path.display(), error = %e, "Failed to canonicalize path");
                return Err(acp::Error::invalid_params());
            }
        };

        if !canonical.starts_with(&self.working_dir) {
            tracing::warn!(path = %args.path.display(), "Read attempt outside working directory");
            return Err(acp::Error::invalid_params());
        }

        let content = match std::fs::read_to_string(&canonical) {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!(path = %args.path.display(), error = %e, "Failed to read file");
                return Err(acp::Error::invalid_params());
            }
        };

        tracing::debug!(path = %args.path.display(), len = content.len(), "Read file");
        Ok(acp::ReadTextFileResponse::new(content))
    }

    async fn create_terminal(
        &self,
        _args: acp::CreateTerminalRequest,
    ) -> acp::Result<acp::CreateTerminalResponse> {
        use std::process::Stdio;

        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());

        let child = match tokio::process::Command::new(&shell)
            .current_dir(&self.working_dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
        {
            Ok(c) => c,
            Err(e) => {
                tracing::error!(error = %e, "Failed to spawn terminal");
                return Err(acp::Error::internal_error());
            }
        };

        let terminal_id = format!("term-{}", child.id().unwrap_or(0));
        tracing::info!(terminal_id = %terminal_id, shell = %shell, "Created terminal");

        Ok(acp::CreateTerminalResponse::new(acp::TerminalId::new(
            terminal_id,
        )))
    }

    async fn terminal_output(
        &self,
        _args: acp::TerminalOutputRequest,
    ) -> acp::Result<acp::TerminalOutputResponse> {
        tracing::debug!("terminal_output called - not fully implemented");
        Ok(acp::TerminalOutputResponse::new(String::new(), false))
    }

    async fn release_terminal(
        &self,
        args: acp::ReleaseTerminalRequest,
    ) -> acp::Result<acp::ReleaseTerminalResponse> {
        tracing::debug!(terminal_id = %args.terminal_id, "Releasing terminal");
        Ok(acp::ReleaseTerminalResponse::new())
    }

    async fn wait_for_terminal_exit(
        &self,
        args: acp::WaitForTerminalExitRequest,
    ) -> acp::Result<acp::WaitForTerminalExitResponse> {
        tracing::debug!(terminal_id = %args.terminal_id, "Waiting for terminal exit");
        Ok(acp::WaitForTerminalExitResponse::new(
            acp::TerminalExitStatus::new(),
        ))
    }

    async fn kill_terminal_command(
        &self,
        args: acp::KillTerminalCommandRequest,
    ) -> acp::Result<acp::KillTerminalCommandResponse> {
        tracing::debug!(terminal_id = %args.terminal_id, "Killing terminal");
        Ok(acp::KillTerminalCommandResponse::new())
    }

    async fn ext_method(&self, _args: acp::ExtRequest) -> acp::Result<acp::ExtResponse> {
        Err(acp::Error::method_not_found())
    }

    async fn ext_notification(&self, _args: acp::ExtNotification) -> acp::Result<()> {
        Ok(())
    }
}

/// ACP client for communicating with an agent process
struct AcpClient {
    child: Child,
    conn: acp::ClientSideConnection,
    event_tx: Arc<std::sync::RwLock<mpsc::Sender<AgentEvent>>>,
    working_dir: PathBuf,
}

impl Drop for AcpClient {
    fn drop(&mut self) {
        if let Err(e) = self.child.start_kill() {
            tracing::warn!(error = %e, "Failed to kill ACP agent process during Drop");
        }
    }
}

impl AcpClient {
    async fn spawn(
        working_dir: &Path,
        agent_binary: &str,
        event_tx: mpsc::Sender<AgentEvent>,
        env_vars: &HashMap<String, String>,
    ) -> Result<Self> {
        if agent_binary.contains("..") || agent_binary.contains('\0') {
            anyhow::bail!("Invalid agent binary path");
        }
        if !working_dir.exists() {
            anyhow::bail!(
                "Working directory does not exist: {}",
                working_dir.display()
            );
        }

        tracing::info!(binary = %agent_binary, cwd = %working_dir.display(), "Spawning ACP agent");

        let mut child = ProcessCommand::new(agent_binary)
            .current_dir(working_dir)
            .envs(env_vars)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::inherit())
            .kill_on_drop(true)
            .spawn()
            .context("Failed to spawn ACP agent")?;

        let stdin = child.stdin.take().context("Failed to get stdin")?;
        let stdout = child.stdout.take().context("Failed to get stdout")?;

        let shared_event_tx = Arc::new(std::sync::RwLock::new(event_tx));

        let handler =
            AcpClientHandler::new(Arc::clone(&shared_event_tx), working_dir.to_path_buf());

        let (conn, handle_io) =
            acp::ClientSideConnection::new(handler, stdin.compat_write(), stdout.compat(), |fut| {
                tokio::task::spawn_local(fut);
            });

        tokio::task::spawn_local(handle_io);

        Ok(Self {
            child,
            conn,
            event_tx: shared_event_tx,
            working_dir: working_dir.to_path_buf(),
        })
    }

    async fn initialize(&self) -> Result<()> {
        self.conn
            .initialize(
                acp::InitializeRequest::new(acp::ProtocolVersion::V1)
                    .client_capabilities(acp::ClientCapabilities::default())
                    .client_info(
                        acp::Implementation::new("gorp-agent-acp", env!("CARGO_PKG_VERSION"))
                            .title("gorp Agent ACP Backend"),
                    ),
            )
            .await
            .context("ACP initialization failed")?;

        tracing::info!("ACP connection initialized");
        Ok(())
    }

    async fn new_session(&self) -> Result<String> {
        tracing::info!(cwd = %self.working_dir.display(), "Calling ACP new_session");
        let response = self
            .conn
            .new_session(acp::NewSessionRequest::new(self.working_dir.clone()))
            .await
            .context("Failed to create new ACP session")?;

        let session_id = response.session_id.to_string();
        tracing::info!(session_id = %session_id, "Created new ACP session");

        let tx = self.event_tx.read().unwrap();
        let _ = tx.try_send(AgentEvent::SessionChanged {
            new_session_id: session_id.clone(),
        });

        Ok(session_id)
    }

    async fn load_session(&self, session_id: &str) -> Result<()> {
        self.conn
            .load_session(acp::LoadSessionRequest::new(
                acp::SessionId::new(session_id.to_string()),
                self.working_dir.clone(),
            ))
            .await
            .context("Failed to load ACP session")?;

        tracing::info!(session_id = %session_id, "Loaded existing ACP session");
        Ok(())
    }

    async fn prompt(&self, session_id: &str, text: &str) -> Result<()> {
        tracing::debug!(session_id = %session_id, prompt_len = text.len(), "Sending prompt");

        let result = self
            .conn
            .prompt(acp::PromptRequest::new(
                acp::SessionId::new(session_id.to_string()),
                vec![acp::ContentBlock::Text(acp::TextContent::new(
                    text.to_string(),
                ))],
            ))
            .await;

        match result {
            Ok(response) => {
                let final_text = format!("Completed: {:?}", response.stop_reason);
                let tx = self.event_tx.read().unwrap();
                let _ = tx.try_send(AgentEvent::Result {
                    text: final_text,
                    usage: None,
                    metadata: serde_json::json!({}),
                });
                Ok(())
            }
            Err(e) => {
                let error_msg = format!("ACP prompt error: {}", e);
                tracing::error!(%error_msg);
                let tx = self.event_tx.read().unwrap();
                let _ = tx.try_send(AgentEvent::Error {
                    code: ErrorCode::BackendError,
                    message: error_msg.clone(),
                    recoverable: false,
                });
                Err(anyhow::anyhow!(error_msg))
            }
        }
    }

    async fn cancel(&self, session_id: &str) -> Result<()> {
        self.conn
            .cancel(acp::CancelNotification::new(acp::SessionId::new(
                session_id.to_string(),
            )))
            .await
            .context("Failed to cancel ACP operation")?;
        Ok(())
    }
}

/// Helper to poll cancellation flag periodically
async fn wait_for_cancellation(cancelled: &AtomicBool) {
    loop {
        if cancelled.load(Ordering::SeqCst) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// Synchronous function to run the ACP operation inside spawn_blocking
#[allow(clippy::too_many_arguments)]
fn run_acp_worker(
    event_tx: mpsc::Sender<AgentEvent>,
    cancelled: Arc<AtomicBool>,
    working_dir: PathBuf,
    agent_binary: String,
    session_id_owned: Option<String>,
    started: bool,
    prompt_text: String,
    env_vars: HashMap<String, String>,
) -> Result<Option<String>> {
    if cancelled.load(Ordering::SeqCst) {
        return Err(anyhow::anyhow!("ACP operation cancelled before start"));
    }

    let rt = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(e) => {
            tracing::error!(error = %e, "Failed to create tokio runtime for ACP invocation");
            let _ = event_tx.try_send(AgentEvent::Error {
                code: ErrorCode::BackendError,
                message: format!("Failed to create runtime: {}", e),
                recoverable: false,
            });
            return Err(anyhow::anyhow!("Failed to create runtime: {}", e));
        }
    };

    rt.block_on(async {
        let local = tokio::task::LocalSet::new();
        local
            .run_until(async {
                let client =
                    match AcpClient::spawn(&working_dir, &agent_binary, event_tx.clone(), &env_vars).await {
                        Ok(c) => c,
                        Err(e) => {
                            tracing::error!(error = %e, "Failed to spawn ACP client");
                            let _ = event_tx.try_send(AgentEvent::Error {
                                code: ErrorCode::BackendError,
                                message: format!("Failed to spawn ACP client: {}", e),
                                recoverable: false,
                            });
                            return Err(e);
                        }
                    };

                if cancelled.load(Ordering::SeqCst) {
                    tracing::info!("ACP operation cancelled after spawn");
                    return Err(anyhow::anyhow!("ACP operation cancelled"));
                }

                if let Err(e) = client.initialize().await {
                    tracing::error!(error = %e, "Failed to initialize ACP connection");
                    let _ = event_tx.try_send(AgentEvent::Error {
                        code: ErrorCode::BackendError,
                        message: format!("Failed to initialize ACP: {}", e),
                        recoverable: false,
                    });
                    return Err(e);
                }

                if cancelled.load(Ordering::SeqCst) {
                    tracing::info!("ACP operation cancelled after initialize");
                    return Err(anyhow::anyhow!("ACP operation cancelled"));
                }

                let active_session_id = if !started {
                    match client.new_session().await {
                        Ok(new_id) => {
                            tracing::info!(session_id = %new_id, "Created new ACP session");
                            new_id
                        }
                        Err(e) => {
                            tracing::error!(error = %e, "Failed to create new ACP session");
                            let _ = event_tx.try_send(AgentEvent::Error {
                                code: ErrorCode::BackendError,
                                message: format!("Failed to create session: {}", e),
                                recoverable: false,
                            });
                            return Err(e);
                        }
                    }
                } else {
                    let existing_session_id = session_id_owned.clone().unwrap_or_default();
                    if let Err(e) = client.load_session(&existing_session_id).await {
                        tracing::warn!(error = %e, session_id = %existing_session_id, "Failed to load existing session, will create new one");
                        match client.new_session().await {
                            Ok(new_id) => {
                                tracing::info!(session_id = %new_id, "Created new ACP session after load failure");
                                let _ = event_tx.try_send(AgentEvent::SessionInvalid {
                                    reason: format!("Original session {} not found", existing_session_id),
                                });
                                new_id
                            }
                            Err(e2) => {
                                tracing::error!(error = %e2, "Failed to create fallback session");
                                let _ = event_tx.try_send(AgentEvent::Error {
                                    code: ErrorCode::SessionOrphaned,
                                    message: format!("Failed to create session: {}", e2),
                                    recoverable: false,
                                });
                                return Err(e2);
                            }
                        }
                    } else {
                        existing_session_id.clone()
                    }
                };

                if cancelled.load(Ordering::SeqCst) {
                    tracing::info!("ACP operation cancelled before prompt");
                    return Err(anyhow::anyhow!("ACP operation cancelled"));
                }

                let prompt_result = tokio::select! {
                    result = client.prompt(&active_session_id, &prompt_text) => {
                        result
                    }
                    _ = wait_for_cancellation(&cancelled) => {
                        tracing::info!("ACP operation cancelled during prompt, sending cancel notification");
                        let _ = client.cancel(&active_session_id).await;
                        return Err(anyhow::anyhow!("ACP operation cancelled during prompt"));
                    }
                };

                prompt_result?;

                if !started {
                    Ok(Some(active_session_id))
                } else {
                    Ok(None)
                }
            })
            .await
    })
}

/// ACP backend implementation
pub struct AcpBackend {
    config: AcpConfig,
}

impl AcpBackend {
    /// Create a new ACP backend with the given config
    pub fn new(config: AcpConfig) -> Result<Self> {
        Ok(Self { config })
    }

    /// Create an AgentHandle that communicates with this backend
    pub fn into_handle(self) -> AgentHandle {
        let (tx, mut rx) = mpsc::channel::<Command>(32);
        let name = "acp";
        let config = self.config;

        // Spawn worker task that handles commands
        tokio::spawn(async move {
            // Capture environment variables for child processes
            let env_vars: HashMap<String, String> = std::env::vars().collect();

            while let Some(cmd) = rx.recv().await {
                match cmd {
                    Command::NewSession { reply } => {
                        // For new session, we'll create it when the first prompt arrives
                        // For now, just generate a placeholder ID
                        let session_id = uuid::Uuid::new_v4().to_string();
                        let _ = reply.send(Ok(session_id));
                    }
                    Command::LoadSession { reply, .. } => {
                        // Session loading happens during prompt
                        let _ = reply.send(Ok(()));
                    }
                    Command::Prompt {
                        session_id,
                        text,
                        event_tx,
                        reply,
                    } => {
                        // Acknowledge prompt started
                        let _ = reply.send(Ok(()));

                        // Determine if this is a new session or existing one
                        let started = !session_id.contains("00000000-0000-0000-0000");
                        let session_id_opt = if started {
                            Some(session_id.clone())
                        } else {
                            None
                        };

                        let cancelled = Arc::new(AtomicBool::new(false));
                        let cancelled_for_task = Arc::clone(&cancelled);
                        let cancelled_for_timeout = Arc::clone(&cancelled);

                        let working_dir = config.working_dir.clone();
                        let agent_binary = config.binary.clone();
                        let prompt_text = text.clone();
                        let event_tx_for_timeout = event_tx.clone();
                        let timeout_secs = config.timeout_secs;
                        let env_vars_clone = env_vars.clone();

                        // Spawn the blocking task
                        let blocking_handle = tokio::task::spawn_blocking(move || {
                            run_acp_worker(
                                event_tx,
                                cancelled_for_task,
                                working_dir,
                                agent_binary,
                                session_id_opt,
                                started,
                                prompt_text,
                                env_vars_clone,
                            )
                        });

                        // Race between completion and timeout
                        tokio::select! {
                            result = blocking_handle => {
                                match result {
                                    Ok(inner) => {
                                        if let Err(e) = inner {
                                            tracing::error!(error = %e, "ACP worker failed");
                                        }
                                    }
                                    Err(e) if e.is_cancelled() => {
                                        tracing::debug!("ACP blocking task was cancelled");
                                    }
                                    Err(e) => {
                                        tracing::error!(error = %e, "ACP blocking task panicked");
                                    }
                                }
                            }
                            _ = tokio::time::sleep(Duration::from_secs(timeout_secs)) => {
                                cancelled_for_timeout.store(true, Ordering::SeqCst);
                                let _ = event_tx_for_timeout.try_send(AgentEvent::Error {
                                    code: ErrorCode::Timeout,
                                    message: format!("ACP operation timed out after {} seconds", timeout_secs),
                                    recoverable: false,
                                });
                                tracing::error!(timeout_secs, "ACP operation timed out");
                            }
                        }
                    }
                    Command::Cancel { session_id, reply } => {
                        tracing::debug!(session_id = %session_id, "Cancel requested for ACP session");
                        // Cancellation is handled via the cancelled flag in the worker
                        let _ = reply.send(Ok(()));
                    }
                }
            }
        });

        AgentHandle::new(tx, name)
    }

    /// Factory function for the registry
    pub fn factory() -> crate::registry::BackendFactory {
        Box::new(|config| {
            let cfg: AcpConfig = serde_json::from_value(config.clone())?;
            let backend = AcpBackend::new(cfg)?;
            Ok(backend.into_handle())
        })
    }
}
