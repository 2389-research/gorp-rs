// ABOUTME: This module provides an ACP (Agent Client Protocol) client for communicating with AI agents.
// ABOUTME: It replaces direct Claude CLI spawning with the standardized ACP protocol over stdio.

use acp::Agent as _;
use agent_client_protocol as acp;
use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::process::{Child, Command};
use tokio::sync::mpsc;
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

/// Events emitted during ACP agent execution
#[derive(Debug, Clone, serde::Serialize)]
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
    /// Session ID changed (new session created)
    SessionChanged { new_session_id: String },
}

/// Handler for ACP client-side callbacks
/// Sends events directly to the provided channel for true streaming
struct AcpClientHandler {
    event_tx: mpsc::Sender<AcpEvent>,
    working_dir: PathBuf,
}

impl AcpClientHandler {
    fn new(event_tx: mpsc::Sender<AcpEvent>, working_dir: PathBuf) -> Self {
        Self {
            event_tx,
            working_dir,
        }
    }

    fn send_event(&self, event: AcpEvent) {
        // Use try_send for non-blocking behavior
        // With a large buffer (2048), this should rarely fail
        if let Err(e) = self.event_tx.try_send(event) {
            match e {
                mpsc::error::TrySendError::Full(dropped_event) => {
                    tracing::warn!(
                        event = ?dropped_event,
                        "Event channel buffer full (2048), dropping event"
                    );
                }
                mpsc::error::TrySendError::Closed(_) => {
                    // Channel closed - receiver dropped, this is expected during shutdown
                    tracing::debug!("Event channel closed, receiver dropped");
                }
            }
        }
    }

    fn log_event_sync(&self, event: &AcpEvent) {
        // Synchronous version for use in the blocking context
        let gorp_dir = self.working_dir.join(".gorp");
        if std::fs::create_dir_all(&gorp_dir).is_err() {
            return;
        }

        let log_path = gorp_dir.join("acp-messages.jsonl");
        let line = match serde_json::to_string(&event) {
            Ok(json) => format!("{}\n", json),
            Err(_) => return,
        };

        use std::io::Write;
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
        {
            let _ = file.write_all(line.as_bytes());
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
            // No options available, return cancelled
            Ok(acp::RequestPermissionResponse::new(
                acp::RequestPermissionOutcome::Cancelled,
            ))
        }
    }

    async fn session_notification(&self, args: acp::SessionNotification) -> acp::Result<()> {
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
                    let event = AcpEvent::Text(text);
                    self.log_event_sync(&event);
                    self.send_event(event);
                }
            }
            acp::SessionUpdate::ToolCall(tool_call) => {
                let name = tool_call.title.clone();
                let preview = tool_call
                    .raw_input
                    .as_ref()
                    .and_then(|v| v.as_object())
                    .and_then(|o| o.get("command").or(o.get("file_path")).or(o.get("pattern")))
                    .and_then(|v| v.as_str())
                    .map(|s| s.chars().take(50).collect())
                    .unwrap_or_default();
                let event = AcpEvent::ToolUse {
                    name,
                    input_preview: preview,
                };
                self.log_event_sync(&event);
                self.send_event(event);
            }
            _ => {}
        }
        Ok(())
    }

    async fn write_text_file(
        &self,
        _args: acp::WriteTextFileRequest,
    ) -> acp::Result<acp::WriteTextFileResponse> {
        Err(acp::Error::method_not_found())
    }

    async fn read_text_file(
        &self,
        _args: acp::ReadTextFileRequest,
    ) -> acp::Result<acp::ReadTextFileResponse> {
        Err(acp::Error::method_not_found())
    }

    async fn create_terminal(
        &self,
        _args: acp::CreateTerminalRequest,
    ) -> acp::Result<acp::CreateTerminalResponse> {
        Err(acp::Error::method_not_found())
    }

    async fn terminal_output(
        &self,
        _args: acp::TerminalOutputRequest,
    ) -> acp::Result<acp::TerminalOutputResponse> {
        Err(acp::Error::method_not_found())
    }

    async fn release_terminal(
        &self,
        _args: acp::ReleaseTerminalRequest,
    ) -> acp::Result<acp::ReleaseTerminalResponse> {
        Err(acp::Error::method_not_found())
    }

    async fn wait_for_terminal_exit(
        &self,
        _args: acp::WaitForTerminalExitRequest,
    ) -> acp::Result<acp::WaitForTerminalExitResponse> {
        Err(acp::Error::method_not_found())
    }

    async fn kill_terminal_command(
        &self,
        _args: acp::KillTerminalCommandRequest,
    ) -> acp::Result<acp::KillTerminalCommandResponse> {
        Err(acp::Error::method_not_found())
    }

    async fn ext_method(&self, _args: acp::ExtRequest) -> acp::Result<acp::ExtResponse> {
        Err(acp::Error::method_not_found())
    }

    async fn ext_notification(&self, _args: acp::ExtNotification) -> acp::Result<()> {
        Ok(())
    }
}

/// ACP client for communicating with an agent process
pub struct AcpClient {
    child: Child,
    conn: acp::ClientSideConnection,
    event_tx: mpsc::Sender<AcpEvent>,
    working_dir: PathBuf,
}

impl Drop for AcpClient {
    fn drop(&mut self) {
        // Explicitly kill the child process when the client is dropped
        // This ensures the agent process is cleaned up even if the client is dropped unexpectedly
        if let Err(e) = self.child.start_kill() {
            tracing::warn!(error = %e, "Failed to kill ACP agent process during Drop");
        }
    }
}

impl AcpClient {
    /// Spawn a new agent process and establish ACP connection
    pub async fn spawn(
        working_dir: &Path,
        agent_binary: &str,
        event_tx: mpsc::Sender<AcpEvent>,
        env_vars: &HashMap<String, String>,
    ) -> Result<Self> {
        // Validate inputs
        if agent_binary.contains("..") || agent_binary.contains('\0') {
            anyhow::bail!("Invalid agent binary path");
        }
        if !working_dir.exists() {
            anyhow::bail!(
                "Working directory does not exist: {}",
                working_dir.display()
            );
        }

        // Log PATH for debugging spawn issues
        if let Some(path) = env_vars.get("PATH") {
            tracing::debug!(path = %path, "PATH being passed to child process");
        } else {
            tracing::warn!("No PATH in env_vars!");
        }

        tracing::info!(binary = %agent_binary, cwd = %working_dir.display(), "Spawning ACP agent");

        let mut child = Command::new(agent_binary)
            .current_dir(working_dir)
            .envs(env_vars)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .context("Failed to spawn ACP agent")?;

        let stdin = child.stdin.take().context("Failed to get stdin")?;
        let stdout = child.stdout.take().context("Failed to get stdout")?;

        let handler = AcpClientHandler::new(event_tx.clone(), working_dir.to_path_buf());

        // Create ACP connection
        let (conn, handle_io) =
            acp::ClientSideConnection::new(handler, stdin.compat_write(), stdout.compat(), |fut| {
                tokio::task::spawn_local(fut);
            });

        // Spawn I/O handler
        tokio::task::spawn_local(handle_io);

        Ok(Self {
            child,
            conn,
            event_tx,
            working_dir: working_dir.to_path_buf(),
        })
    }

    /// Initialize the ACP connection
    pub async fn initialize(&self) -> Result<()> {
        self.conn
            .initialize(
                acp::InitializeRequest::new(acp::ProtocolVersion::V1)
                    .client_capabilities(acp::ClientCapabilities::default())
                    .client_info(
                        acp::Implementation::new("gorp-acp", env!("CARGO_PKG_VERSION"))
                            .title("Matrix-Claude Bridge"),
                    ),
            )
            .await
            .context("ACP initialization failed")?;

        tracing::info!("ACP connection initialized");
        Ok(())
    }

    /// Create a new session
    pub async fn new_session(&self) -> Result<String> {
        let response = self
            .conn
            .new_session(acp::NewSessionRequest::new(self.working_dir.clone()))
            .await
            .context("Failed to create new ACP session")?;

        let session_id = response.session_id.to_string();
        tracing::info!(session_id = %session_id, "Created new ACP session");

        // Notify about the new session ID
        let _ = self.event_tx.try_send(AcpEvent::SessionChanged {
            new_session_id: session_id.clone(),
        });

        Ok(session_id)
    }

    /// Load an existing session by ID
    pub async fn load_session(&self, session_id: &str) -> Result<()> {
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

    /// Send a prompt - events stream via session_notification callback
    pub async fn prompt(&self, session_id: &str, text: &str) -> Result<()> {
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
                // The response only contains stop_reason; content is streamed via session_notification
                let final_text = format!("Completed: {:?}", response.stop_reason);
                let _ = self
                    .event_tx
                    .try_send(AcpEvent::Result { text: final_text });
                Ok(())
            }
            Err(e) => {
                let error_msg = format!("ACP prompt error: {}", e);
                tracing::error!(%error_msg);
                let _ = self.event_tx.try_send(AcpEvent::Error(error_msg.clone()));
                Err(anyhow::anyhow!(error_msg))
            }
        }
    }

    /// Cancel the current operation
    pub async fn cancel(&self, session_id: &str) -> Result<()> {
        self.conn
            .cancel(acp::CancelNotification::new(acp::SessionId::new(
                session_id.to_string(),
            )))
            .await
            .context("Failed to cancel ACP operation")?;
        Ok(())
    }
}

/// Handle for a running ACP task, allowing cancellation and cleanup
pub struct AcpTaskHandle {
    cancelled: Arc<AtomicBool>,
    task_handle: Option<tokio::task::JoinHandle<Result<Option<String>>>>,
}

impl AcpTaskHandle {
    /// Cancel the ACP task - signals the task to stop and kills the child process
    pub fn cancel(&mut self) {
        self.cancelled.store(true, Ordering::SeqCst);
        if let Some(handle) = self.task_handle.take() {
            handle.abort();
        }
    }

    /// Wait for the task to complete and get the final session ID (if a new one was created)
    pub async fn wait(mut self) -> Result<Option<String>> {
        if let Some(handle) = self.task_handle.take() {
            match handle.await {
                Ok(result) => result,
                Err(e) if e.is_cancelled() => Err(anyhow::anyhow!("ACP task was cancelled")),
                Err(e) => Err(anyhow::anyhow!("ACP task panicked: {}", e)),
            }
        } else {
            Err(anyhow::anyhow!("ACP task handle was already consumed"))
        }
    }
}

impl Drop for AcpTaskHandle {
    fn drop(&mut self) {
        // Signal cancellation and abort the task if still running
        self.cancelled.store(true, Ordering::SeqCst);
        if let Some(handle) = self.task_handle.take() {
            handle.abort();
        }
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

/// Invoke ACP agent with streaming - returns immediately with event receiver
///
/// This function spawns the ACP task and returns immediately, allowing the caller
/// to start consuming events while the ACP agent is still running. This enables
/// true streaming of events instead of buffering everything.
///
/// Returns:
/// - Event receiver for streaming events
/// - Task handle for cancellation and waiting for completion
///
/// The task handle should be awaited with `.wait()` after consuming all events
/// to get the final session ID (if a new one was created).
pub async fn invoke_acp(
    agent_binary: &str,
    working_dir: &Path,
    session_id: Option<&str>,
    started: bool,
    prompt: &str,
    timeout_secs: u64,
) -> Result<(mpsc::Receiver<AcpEvent>, AcpTaskHandle)> {
    // Large buffer to prevent event loss during streaming
    // 2048 should be enough for even very long responses with many tool calls
    let (event_tx, event_rx) = mpsc::channel(2048);

    // Cancellation flag shared between caller and task
    let cancelled = Arc::new(AtomicBool::new(false));
    let cancelled_for_task = Arc::clone(&cancelled);
    let cancelled_for_timeout = Arc::clone(&cancelled);

    let working_dir = working_dir.to_path_buf();
    let agent_binary = agent_binary.to_string();
    let session_id_owned = session_id.map(|s| s.to_string());
    let prompt_text = prompt.to_string();
    let event_tx_for_timeout = event_tx.clone();

    // Capture environment variables BEFORE entering spawn_blocking
    // This ensures PATH and other env vars are available to the child process
    let env_vars: HashMap<String, String> = std::env::vars().collect();

    // Log PATH at invoke time for debugging
    if let Some(path) = env_vars.get("PATH") {
        tracing::info!(path_len = path.len(), "Captured PATH for ACP spawn");
        tracing::debug!(path = %path, "Full PATH value");
    } else {
        tracing::error!("No PATH environment variable found!");
    }

    // Spawn the ACP task - returns immediately
    let task_handle = tokio::task::spawn(async move {
        let timeout_duration = Duration::from_secs(timeout_secs);

        // Spawn the blocking task
        let blocking_handle = tokio::task::spawn_blocking(move || {
            run_acp_sync(
                event_tx,
                cancelled_for_task,
                working_dir,
                agent_binary,
                session_id_owned,
                started,
                prompt_text,
                env_vars,
            )
        });

        // Race between: blocking task completion, timeout, and external cancellation
        tokio::select! {
            result = blocking_handle => {
                // Normal completion
                match result {
                    Ok(inner) => inner,
                    Err(e) if e.is_cancelled() => {
                        Err(anyhow::anyhow!("ACP blocking task was cancelled"))
                    }
                    Err(e) => {
                        Err(anyhow::anyhow!("ACP blocking task panicked: {}", e))
                    }
                }
            }
            _ = tokio::time::sleep(timeout_duration) => {
                // Timeout - signal cancellation so blocking task stops
                cancelled_for_timeout.store(true, Ordering::SeqCst);
                let _ = event_tx_for_timeout.try_send(AcpEvent::Error(format!(
                    "ACP operation timed out after {} seconds",
                    timeout_secs
                )));
                Err(anyhow::anyhow!(
                    "ACP operation timed out after {} seconds",
                    timeout_secs
                ))
            }
        }
    });

    Ok((
        event_rx,
        AcpTaskHandle {
            cancelled,
            task_handle: Some(task_handle),
        },
    ))
}

/// Synchronous function to run the ACP operation inside spawn_blocking
#[allow(clippy::too_many_arguments)]
fn run_acp_sync(
    event_tx: mpsc::Sender<AcpEvent>,
    cancelled: Arc<AtomicBool>,
    working_dir: PathBuf,
    agent_binary: String,
    session_id_owned: Option<String>,
    started: bool,
    prompt_text: String,
    env_vars: HashMap<String, String>,
) -> Result<Option<String>> {
    // Check cancellation before starting
    if cancelled.load(Ordering::SeqCst) {
        return Err(anyhow::anyhow!("ACP operation cancelled before start"));
    }

    let rt = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(e) => {
            tracing::error!(error = %e, "Failed to create tokio runtime for ACP invocation");
            let _ = event_tx.try_send(AcpEvent::Error(format!("Failed to create runtime: {}", e)));
            return Err(anyhow::anyhow!("Failed to create runtime: {}", e));
        }
    };

    rt.block_on(async {
        let local = tokio::task::LocalSet::new();
        local
            .run_until(async {
                // Spawn ACP client with the event sender
                let client =
                    match AcpClient::spawn(&working_dir, &agent_binary, event_tx.clone(), &env_vars).await {
                        Ok(c) => c,
                        Err(e) => {
                            tracing::error!(error = %e, "Failed to spawn ACP client");
                            let _ = event_tx.try_send(AcpEvent::Error(format!(
                                "Failed to spawn ACP client: {}",
                                e
                            )));
                            return Err(e);
                        }
                    };

                // Check cancellation
                if cancelled.load(Ordering::SeqCst) {
                    tracing::info!("ACP operation cancelled after spawn");
                    return Err(anyhow::anyhow!("ACP operation cancelled"));
                }

                // Initialize ACP connection
                if let Err(e) = client.initialize().await {
                    tracing::error!(error = %e, "Failed to initialize ACP connection");
                    let _ = event_tx
                        .try_send(AcpEvent::Error(format!("Failed to initialize ACP: {}", e)));
                    return Err(e);
                }

                // Check cancellation
                if cancelled.load(Ordering::SeqCst) {
                    tracing::info!("ACP operation cancelled after initialize");
                    return Err(anyhow::anyhow!("ACP operation cancelled"));
                }

                // Create or load session
                let active_session_id = if !started {
                    // New session - create it (event is sent inside new_session)
                    match client.new_session().await {
                        Ok(new_id) => {
                            tracing::info!(session_id = %new_id, "Created new ACP session");
                            new_id
                        }
                        Err(e) => {
                            tracing::error!(error = %e, "Failed to create new ACP session");
                            let _ = event_tx.try_send(AcpEvent::Error(format!(
                                "Failed to create session: {}",
                                e
                            )));
                            return Err(e);
                        }
                    }
                } else {
                    // Existing session - load it
                    let existing_session_id = session_id_owned.clone().unwrap_or_default();
                    if let Err(e) = client.load_session(&existing_session_id).await {
                        tracing::warn!(error = %e, session_id = %existing_session_id, "Failed to load existing session, will create new one");
                        // Try creating new session instead
                        match client.new_session().await {
                            Ok(new_id) => {
                                tracing::info!(session_id = %new_id, "Created new ACP session after load failure");
                                new_id
                            }
                            Err(e2) => {
                                tracing::error!(error = %e2, "Failed to create fallback session");
                                let _ = event_tx.try_send(AcpEvent::Error(format!(
                                    "Failed to create session: {}",
                                    e2
                                )));
                                return Err(e2);
                            }
                        }
                    } else {
                        existing_session_id.clone()
                    }
                };

                // Check cancellation before sending prompt
                if cancelled.load(Ordering::SeqCst) {
                    tracing::info!("ACP operation cancelled before prompt");
                    return Err(anyhow::anyhow!("ACP operation cancelled"));
                }

                // Send prompt with cancellation support
                // Use select! to race prompt against cancellation check
                let prompt_result = tokio::select! {
                    result = client.prompt(&active_session_id, &prompt_text) => {
                        result
                    }
                    _ = wait_for_cancellation(&cancelled) => {
                        // Cancelled during prompt - try to send cancel notification
                        tracing::info!("ACP operation cancelled during prompt, sending cancel notification");
                        let _ = client.cancel(&active_session_id).await;
                        // Client will be dropped here, killing the child process
                        return Err(anyhow::anyhow!("ACP operation cancelled during prompt"));
                    }
                };

                // Handle prompt result
                prompt_result?;

                // Return the session ID if it was newly created
                if !started {
                    Ok(Some(active_session_id))
                } else {
                    Ok(None)
                }
            })
            .await
    })
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

        let event = AcpEvent::Result {
            text: "done".to_string(),
        };
        assert!(matches!(event, AcpEvent::Result { .. }));
    }
}
