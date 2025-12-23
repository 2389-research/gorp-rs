// ABOUTME: ACP protocol backend - communicates with claude-code-acp or codex-acp.
// ABOUTME: Keeps ACP process alive across prompts for session persistence.

use crate::event::{AgentEvent, ErrorCode};
use crate::handle::{AgentHandle, Command};
use acp::Agent as _;
use agent_client_protocol as acp;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread;
use tokio::process::{Child, Command as ProcessCommand};
use tokio::sync::{mpsc, oneshot};
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
    /// Extra CLI arguments to pass to the ACP binary
    #[serde(default)]
    pub extra_args: Vec<String>,
}

fn default_timeout() -> u64 {
    300 // 5 minutes
}

/// Commands sent to the persistent ACP worker thread
enum WorkerCommand {
    NewSession {
        reply: oneshot::Sender<Result<String, String>>,
    },
    LoadSession {
        session_id: String,
        reply: oneshot::Sender<Result<(), String>>,
    },
    Prompt {
        session_id: String,
        text: String,
        event_tx: mpsc::Sender<AgentEvent>,
    },
    Cancel {
        session_id: String,
    },
    Shutdown,
}

/// Handler for ACP client-side callbacks
/// Sends events directly to the provided channel for true streaming
struct AcpClientHandler {
    event_tx: Arc<std::sync::RwLock<mpsc::Sender<AgentEvent>>>,
    working_dir: PathBuf,
    /// Buffer for accumulating text to parse **status** patterns across chunks
    text_buffer: std::sync::Mutex<String>,
}

impl AcpClientHandler {
    fn new(
        event_tx: Arc<std::sync::RwLock<mpsc::Sender<AgentEvent>>>,
        working_dir: PathBuf,
    ) -> Self {
        Self {
            event_tx,
            working_dir,
            text_buffer: std::sync::Mutex::new(String::new()),
        }
    }

    fn update_event_tx(&self, new_tx: mpsc::Sender<AgentEvent>) {
        let mut tx = self.event_tx.write().unwrap_or_else(|e| e.into_inner());
        *tx = new_tx;
    }

    /// Create a new dummy sender and drop the old one to close the channel.
    /// This signals to the receiver that no more events are coming.
    fn close_event_channel(&self) {
        let (dummy_tx, _dummy_rx) = mpsc::channel(1);
        let mut tx = self.event_tx.write().unwrap_or_else(|e| e.into_inner());
        *tx = dummy_tx;
        // Old tx is dropped here, closing the channel for the receiver
    }

    fn send_event(&self, event: AgentEvent) {
        let tx = self.event_tx.read().unwrap_or_else(|e| e.into_inner());
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

    /// Buffer text and parse **status** patterns (used by codex).
    /// Emits complete patterns immediately, buffers incomplete ones.
    fn buffer_and_parse_text(&self, text: &str) {
        let mut buffer = self.text_buffer.lock().unwrap_or_else(|e| e.into_inner());
        buffer.push_str(text);

        // Process complete **...** patterns from the buffer
        loop {
            if let Some(start) = buffer.find("**") {
                // Look for closing **
                let after_start = &buffer[start + 2..];
                if let Some(end) = after_start.find("**") {
                    // Found complete pattern
                    // Emit any text before the **
                    let before = &buffer[..start];
                    if !before.is_empty() {
                        self.send_event(AgentEvent::Text(before.to_string()));
                    }

                    // Emit the status as a Custom "thinking" event
                    let status_text = &after_start[..end];
                    self.send_event(AgentEvent::Custom {
                        kind: "thinking".to_string(),
                        payload: serde_json::json!({ "status": status_text }),
                    });

                    // Remove processed text from buffer
                    let consumed = start + 2 + end + 2;
                    *buffer = buffer[consumed..].to_string();
                } else {
                    // Have opening ** but no closing yet - keep buffering
                    // But emit any text before the ** to avoid buffering too much
                    if start > 0 {
                        let before = buffer[..start].to_string();
                        self.send_event(AgentEvent::Text(before));
                        *buffer = buffer[start..].to_string();
                    }
                    break;
                }
            } else {
                // No ** pattern found
                // Check if buffer ends with a single * (might be start of **)
                if buffer.ends_with('*') && buffer.len() > 1 {
                    // Keep the trailing * in buffer, emit the rest
                    let emit_len = buffer.len() - 1;
                    if emit_len > 0 {
                        let to_emit = buffer[..emit_len].to_string();
                        self.send_event(AgentEvent::Text(to_emit));
                        *buffer = buffer[emit_len..].to_string();
                    }
                } else if !buffer.is_empty() && !buffer.contains('*') {
                    // No asterisks at all, safe to emit everything
                    let to_emit = std::mem::take(&mut *buffer);
                    self.send_event(AgentEvent::Text(to_emit));
                }
                break;
            }
        }
    }

    /// Flush any remaining buffered text (call when message is complete)
    fn flush_text_buffer(&self) {
        let mut buffer = self.text_buffer.lock().unwrap_or_else(|e| e.into_inner());
        if !buffer.is_empty() {
            let remaining = std::mem::take(&mut *buffer);
            // Try one more parse in case we have a complete pattern
            if let Some(start) = remaining.find("**") {
                let after_start = &remaining[start + 2..];
                if let Some(end) = after_start.find("**") {
                    if start > 0 {
                        self.send_event(AgentEvent::Text(remaining[..start].to_string()));
                    }
                    self.send_event(AgentEvent::Custom {
                        kind: "thinking".to_string(),
                        payload: serde_json::json!({ "status": &after_start[..end] }),
                    });
                    let after = &after_start[end + 2..];
                    if !after.is_empty() {
                        self.send_event(AgentEvent::Text(after.to_string()));
                    }
                    return;
                }
            }
            // No complete pattern, emit as text
            self.send_event(AgentEvent::Text(remaining));
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
                    // Buffer and parse **status** patterns (used by codex for thinking/status)
                    self.buffer_and_parse_text(&text);
                }
            }
            acp::SessionUpdate::ToolCall(tool_call) => {
                // Flush text buffer before tool call
                self.flush_text_buffer();
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
                    // Buffer and parse **status** patterns (used by codex for thinking/status)
                    self.buffer_and_parse_text(&text);
                }
            }
            other => {
                // Flush text buffer on any other event type
                self.flush_text_buffer();
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

/// Persistent ACP client that stays alive across prompts
struct PersistentAcpClient {
    child: Child,
    conn: acp::ClientSideConnection,
    handler: Arc<AcpClientHandler>,
    working_dir: PathBuf,
    /// Currently active session ID
    current_session: Option<String>,
}

impl Drop for PersistentAcpClient {
    fn drop(&mut self) {
        if let Err(e) = self.child.start_kill() {
            tracing::warn!(error = %e, "Failed to kill ACP agent process during Drop");
        }
    }
}

impl PersistentAcpClient {
    async fn spawn(
        working_dir: &Path,
        agent_binary: &str,
        extra_args: &[String],
        initial_event_tx: mpsc::Sender<AgentEvent>,
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

        tracing::info!(binary = %agent_binary, ?extra_args, cwd = %working_dir.display(), "Spawning persistent ACP agent");

        let mut child = ProcessCommand::new(agent_binary)
            .args(extra_args)
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

        let shared_event_tx = Arc::new(std::sync::RwLock::new(initial_event_tx));
        let handler = Arc::new(AcpClientHandler::new(
            Arc::clone(&shared_event_tx),
            working_dir.to_path_buf(),
        ));

        // Clone handler for the connection (it implements Client)
        let handler_for_conn = HandlerWrapper(Arc::clone(&handler));

        let (conn, handle_io) = acp::ClientSideConnection::new(
            handler_for_conn,
            stdin.compat_write(),
            stdout.compat(),
            |fut| {
                tokio::task::spawn_local(fut);
            },
        );

        tokio::task::spawn_local(handle_io);

        Ok(Self {
            child,
            conn,
            handler,
            working_dir: working_dir.to_path_buf(),
            current_session: None,
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

    async fn new_session(&mut self) -> Result<String> {
        tracing::info!(cwd = %self.working_dir.display(), "Calling ACP new_session");
        let response = self
            .conn
            .new_session(acp::NewSessionRequest::new(self.working_dir.clone()))
            .await
            .context("Failed to create new ACP session")?;

        let session_id = response.session_id.to_string();
        self.current_session = Some(session_id.clone());
        tracing::info!(session_id = %session_id, "Created new ACP session");

        Ok(session_id)
    }

    async fn load_session(&mut self, session_id: &str) -> Result<()> {
        self.conn
            .load_session(acp::LoadSessionRequest::new(
                acp::SessionId::new(session_id.to_string()),
                self.working_dir.clone(),
            ))
            .await
            .context("Failed to load ACP session")?;

        // Session ID remains the same after load
        self.current_session = Some(session_id.to_string());
        tracing::info!(session_id = %session_id, "Loaded ACP session");
        Ok(())
    }

    fn update_event_tx(&self, new_tx: mpsc::Sender<AgentEvent>) {
        self.handler.update_event_tx(new_tx);
    }

    /// Close the event channel to signal that no more events are coming.
    fn close_event_channel(&self) {
        self.handler.close_event_channel();
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

        // Flush any remaining buffered text
        self.handler.flush_text_buffer();

        match result {
            Ok(response) => {
                let final_text = format!("Completed: {:?}", response.stop_reason);
                self.handler.send_event(AgentEvent::Result {
                    text: final_text,
                    usage: None,
                    metadata: serde_json::json!({}),
                });
                Ok(())
            }
            Err(e) => {
                let error_msg = format!("ACP prompt error: {}", e);
                tracing::error!(%error_msg);
                self.handler.send_event(AgentEvent::Error {
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

/// Wrapper to implement acp::Client for Arc<AcpClientHandler>
struct HandlerWrapper(Arc<AcpClientHandler>);

#[async_trait::async_trait(?Send)]
impl acp::Client for HandlerWrapper {
    async fn request_permission(
        &self,
        args: acp::RequestPermissionRequest,
    ) -> acp::Result<acp::RequestPermissionResponse> {
        self.0.request_permission(args).await
    }

    async fn session_notification(&self, args: acp::SessionNotification) -> acp::Result<()> {
        self.0.session_notification(args).await
    }

    async fn write_text_file(
        &self,
        args: acp::WriteTextFileRequest,
    ) -> acp::Result<acp::WriteTextFileResponse> {
        self.0.write_text_file(args).await
    }

    async fn read_text_file(
        &self,
        args: acp::ReadTextFileRequest,
    ) -> acp::Result<acp::ReadTextFileResponse> {
        self.0.read_text_file(args).await
    }

    async fn create_terminal(
        &self,
        args: acp::CreateTerminalRequest,
    ) -> acp::Result<acp::CreateTerminalResponse> {
        self.0.create_terminal(args).await
    }

    async fn terminal_output(
        &self,
        args: acp::TerminalOutputRequest,
    ) -> acp::Result<acp::TerminalOutputResponse> {
        self.0.terminal_output(args).await
    }

    async fn release_terminal(
        &self,
        args: acp::ReleaseTerminalRequest,
    ) -> acp::Result<acp::ReleaseTerminalResponse> {
        self.0.release_terminal(args).await
    }

    async fn wait_for_terminal_exit(
        &self,
        args: acp::WaitForTerminalExitRequest,
    ) -> acp::Result<acp::WaitForTerminalExitResponse> {
        self.0.wait_for_terminal_exit(args).await
    }

    async fn kill_terminal_command(
        &self,
        args: acp::KillTerminalCommandRequest,
    ) -> acp::Result<acp::KillTerminalCommandResponse> {
        self.0.kill_terminal_command(args).await
    }

    async fn ext_method(&self, args: acp::ExtRequest) -> acp::Result<acp::ExtResponse> {
        self.0.ext_method(args).await
    }

    async fn ext_notification(&self, args: acp::ExtNotification) -> acp::Result<()> {
        self.0.ext_notification(args).await
    }
}

/// Run the persistent ACP worker on a dedicated thread
fn run_persistent_worker(config: AcpConfig, mut cmd_rx: mpsc::Receiver<WorkerCommand>) {
    // Create a new runtime for this thread
    let rt = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            tracing::error!(error = %e, "Failed to create tokio runtime for ACP worker");
            return;
        }
    };

    rt.block_on(async {
        let local = tokio::task::LocalSet::new();
        local
            .run_until(async {
                let env_vars: HashMap<String, String> = std::env::vars().collect();

                // Create a dummy channel for initial spawn - will be replaced on first prompt
                let (dummy_tx, _dummy_rx) = mpsc::channel(1);

                // Spawn the ACP client
                let mut client = match PersistentAcpClient::spawn(
                    &config.working_dir,
                    &config.binary,
                    &config.extra_args,
                    dummy_tx,
                    &env_vars,
                )
                .await
                {
                    Ok(c) => c,
                    Err(e) => {
                        tracing::error!(error = %e, "Failed to spawn persistent ACP client");
                        return;
                    }
                };

                // Initialize the connection
                if let Err(e) = client.initialize().await {
                    tracing::error!(error = %e, "Failed to initialize ACP connection");
                    return;
                }

                tracing::info!("Persistent ACP worker started");

                // Process commands
                while let Some(cmd) = cmd_rx.recv().await {
                    match cmd {
                        WorkerCommand::NewSession { reply } => {
                            let result = client.new_session().await;
                            let _ = reply.send(result.map_err(|e| e.to_string()));
                        }
                        WorkerCommand::LoadSession { session_id, reply } => {
                            let result = client.load_session(&session_id).await;
                            let _ = reply.send(result.map_err(|e| e.to_string()));
                        }
                        WorkerCommand::Prompt {
                            session_id,
                            text,
                            event_tx,
                        } => {
                            // Update the event channel for this prompt
                            client.update_event_tx(event_tx.clone());

                            // Send the prompt with timeout
                            let timeout_duration =
                                std::time::Duration::from_secs(config.timeout_secs);
                            match tokio::time::timeout(
                                timeout_duration,
                                client.prompt(&session_id, &text),
                            )
                            .await
                            {
                                Ok(Ok(())) => {
                                    // Prompt completed successfully
                                    tracing::debug!("Prompt completed successfully");
                                }
                                Ok(Err(e)) => {
                                    // ACP error occurred
                                    tracing::error!(error = %e, "Prompt failed");
                                    let _ = event_tx
                                        .send(AgentEvent::Error {
                                            code: ErrorCode::BackendError,
                                            message: format!("ACP prompt error: {}", e),
                                            recoverable: false,
                                        })
                                        .await;
                                }
                                Err(_) => {
                                    // Timeout occurred
                                    tracing::error!(
                                        timeout_secs = config.timeout_secs,
                                        "Prompt timed out"
                                    );
                                    let _ = event_tx
                                        .send(AgentEvent::Error {
                                            code: ErrorCode::Timeout,
                                            message: format!(
                                                "ACP prompt timed out after {} seconds",
                                                config.timeout_secs
                                            ),
                                            recoverable: true,
                                        })
                                        .await;
                                }
                            }

                            // Close the event channel to signal that this prompt is complete.
                            // This causes the receiver's recv() to return None.
                            client.close_event_channel();
                        }
                        WorkerCommand::Cancel { session_id } => {
                            if let Err(e) = client.cancel(&session_id).await {
                                tracing::warn!(error = %e, "Cancel failed");
                            }
                        }
                        WorkerCommand::Shutdown => {
                            tracing::info!("ACP worker shutting down");
                            break;
                        }
                    }
                }
            })
            .await;
    });
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
        let (handle_tx, mut handle_rx) = mpsc::channel::<Command>(32);
        let (worker_tx, worker_rx) = mpsc::channel::<WorkerCommand>(32);
        let name = "acp";
        let config = self.config;

        // Spawn the persistent worker on a dedicated thread
        let worker_config = config.clone();
        thread::spawn(move || {
            run_persistent_worker(worker_config, worker_rx);
        });

        // Spawn the command router that translates Handle commands to Worker commands
        let worker_tx_clone = worker_tx.clone();
        tokio::spawn(async move {
            while let Some(cmd) = handle_rx.recv().await {
                match cmd {
                    Command::NewSession { reply } => {
                        // Create a new session via the worker
                        let (tx, rx) = oneshot::channel();
                        if worker_tx_clone
                            .send(WorkerCommand::NewSession { reply: tx })
                            .await
                            .is_err()
                        {
                            let _ = reply.send(Err(anyhow::anyhow!("Worker channel closed")));
                            continue;
                        }
                        match rx.await {
                            Ok(Ok(session_id)) => {
                                let _ = reply.send(Ok(session_id));
                            }
                            Ok(Err(e)) => {
                                let _ = reply.send(Err(anyhow::anyhow!(e)));
                            }
                            Err(_) => {
                                let _ = reply.send(Err(anyhow::anyhow!("Worker dropped reply")));
                            }
                        }
                    }
                    Command::LoadSession { session_id, reply } => {
                        // Load an existing session
                        let (tx, rx) = oneshot::channel();
                        if worker_tx_clone
                            .send(WorkerCommand::LoadSession {
                                session_id: session_id.clone(),
                                reply: tx,
                            })
                            .await
                            .is_err()
                        {
                            let _ = reply.send(Err(anyhow::anyhow!("Worker channel closed")));
                            continue;
                        }
                        match rx.await {
                            Ok(Ok(())) => {
                                let _ = reply.send(Ok(()));
                            }
                            Ok(Err(e)) => {
                                let _ = reply.send(Err(anyhow::anyhow!(e)));
                            }
                            Err(_) => {
                                let _ = reply.send(Err(anyhow::anyhow!("Worker dropped reply")));
                            }
                        }
                    }
                    Command::Prompt {
                        session_id,
                        text,
                        event_tx,
                        reply,
                        is_new_session: _,
                    } => {
                        // Acknowledge immediately
                        let _ = reply.send(Ok(()));

                        // Send prompt to worker
                        if worker_tx_clone
                            .send(WorkerCommand::Prompt {
                                session_id,
                                text,
                                event_tx,
                            })
                            .await
                            .is_err()
                        {
                            tracing::error!("Failed to send prompt to worker");
                        }
                    }
                    Command::Cancel { session_id, reply } => {
                        if worker_tx_clone
                            .send(WorkerCommand::Cancel { session_id })
                            .await
                            .is_err()
                        {
                            tracing::warn!("Failed to send cancel to worker");
                        }
                        let _ = reply.send(Ok(()));
                    }
                }
            }

            // Shutdown the worker when handle is dropped
            let _ = worker_tx_clone.send(WorkerCommand::Shutdown).await;
        });

        AgentHandle::new(handle_tx, name)
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
