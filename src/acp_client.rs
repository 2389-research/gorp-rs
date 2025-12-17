// ABOUTME: This module provides an ACP (Agent Client Protocol) client for communicating with AI agents.
// ABOUTME: It replaces direct Claude CLI spawning with the standardized ACP protocol over stdio.

use acp::Agent as _;
use agent_client_protocol as acp;
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::process::{Child, Command};
use tokio::sync::{mpsc, Mutex};
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
#[derive(Clone)]
struct AcpClientHandler {
    event_tx: Arc<Mutex<Option<mpsc::Sender<AcpEvent>>>>,
    working_dir: PathBuf,
}

impl AcpClientHandler {
    fn new(working_dir: PathBuf) -> Self {
        Self {
            event_tx: Arc::new(Mutex::new(None)),
            working_dir,
        }
    }

    async fn set_event_sender(&self, tx: mpsc::Sender<AcpEvent>) {
        let mut guard = self.event_tx.lock().await;
        *guard = Some(tx);
    }

    async fn send_event(&self, event: AcpEvent) {
        if let Some(tx) = self.event_tx.lock().await.as_ref() {
            let _ = tx.send(event).await;
        }
    }

    async fn log_event(&self, event: &AcpEvent) {
        let gorp_dir = self.working_dir.join(".gorp");
        if tokio::fs::create_dir_all(&gorp_dir).await.is_err() {
            return;
        }

        let log_path = gorp_dir.join("acp-messages.jsonl");
        let line = match serde_json::to_string(&event) {
            Ok(json) => format!("{}\n", json),
            Err(_) => return,
        };

        if let Ok(mut file) = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
            .await
        {
            use tokio::io::AsyncWriteExt;
            let _ = file.write_all(line.as_bytes()).await;
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
                    self.log_event(&event).await;
                    self.send_event(event).await;
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
                self.log_event(&event).await;
                self.send_event(event).await;
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
    handler: Arc<AcpClientHandler>,
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
    pub async fn spawn(working_dir: &Path, agent_binary: &str) -> Result<Self> {
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

        tracing::info!(binary = %agent_binary, cwd = %working_dir.display(), "Spawning ACP agent");

        let mut child = Command::new(agent_binary)
            .current_dir(working_dir)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .context("Failed to spawn ACP agent")?;

        let stdin = child.stdin.take().context("Failed to get stdin")?;
        let stdout = child.stdout.take().context("Failed to get stdout")?;

        let handler = Arc::new(AcpClientHandler::new(working_dir.to_path_buf()));
        let handler_clone = Arc::clone(&handler);

        // Create ACP connection
        let (conn, handle_io) = acp::ClientSideConnection::new(
            (*handler_clone).clone(),
            stdin.compat_write(),
            stdout.compat(),
            |fut| {
                tokio::task::spawn_local(fut);
            },
        );

        // Spawn I/O handler
        tokio::task::spawn_local(handle_io);

        Ok(Self {
            child,
            conn,
            handler,
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

    /// Send a prompt and receive streaming events
    pub async fn prompt(&self, session_id: &str, text: &str) -> Result<mpsc::Receiver<AcpEvent>> {
        let (tx, rx) = mpsc::channel(32);
        self.handler.set_event_sender(tx.clone()).await;

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
                let _ = tx.send(AcpEvent::Result { text: final_text }).await;
            }
            Err(e) => {
                let error_msg = format!("ACP prompt error: {}", e);
                tracing::error!(%error_msg);
                let _ = tx.send(AcpEvent::Error(error_msg)).await;
            }
        }

        Ok(rx)
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

/// Invoke ACP agent with full spawn → initialize → session → prompt flow
///
/// This encapsulates all the complexity of:
/// - spawn_blocking + LocalSet pattern (required for !Send ACP client)
/// - Spawning and initializing the ACP client
/// - Creating new session or loading existing session
/// - Sending prompt and collecting response events
///
/// Returns the event receiver and optionally a new session ID if one was created.
pub async fn invoke_acp(
    agent_binary: &str,
    working_dir: &Path,
    session_id: Option<&str>,
    started: bool,
    prompt: &str,
) -> Result<(mpsc::Receiver<AcpEvent>, Option<String>)> {
    let (event_tx, event_rx) = mpsc::channel(32);

    let working_dir = working_dir.to_path_buf();
    let agent_binary = agent_binary.to_string();
    let session_id_owned = session_id.map(|s| s.to_string());
    let prompt_text = prompt.to_string();

    // ACP requires spawn_local which isn't Send, so use spawn_blocking with a new runtime
    tokio::task::spawn_blocking(move || {
        let rt = match tokio::runtime::Runtime::new() {
            Ok(rt) => rt,
            Err(e) => {
                tracing::error!(error = %e, "Failed to create tokio runtime for ACP invocation");
                return Err(anyhow::anyhow!("Failed to create runtime: {}", e));
            }
        };

        rt.block_on(async {
            let local = tokio::task::LocalSet::new();
            local.run_until(async move {
                // Spawn ACP client
                let client = match AcpClient::spawn(&working_dir, &agent_binary).await {
                    Ok(c) => c,
                    Err(e) => {
                        tracing::error!(error = %e, "Failed to spawn ACP client");
                        let _ = event_tx.send(AcpEvent::Error(format!("Failed to spawn ACP client: {}", e))).await;
                        return Err(e);
                    }
                };

                // Initialize ACP connection
                if let Err(e) = client.initialize().await {
                    tracing::error!(error = %e, "Failed to initialize ACP connection");
                    let _ = event_tx.send(AcpEvent::Error(format!("Failed to initialize ACP: {}", e))).await;
                    return Err(e);
                }

                // Create or load session
                let (active_session_id, session_changed) = if !started {
                    // New session - create it
                    match client.new_session().await {
                        Ok(new_id) => {
                            tracing::info!(session_id = %new_id, "Created new ACP session");
                            // Notify that session ID changed
                            let _ = event_tx.send(AcpEvent::SessionChanged { new_session_id: new_id.clone() }).await;
                            (new_id, true)
                        }
                        Err(e) => {
                            tracing::error!(error = %e, "Failed to create new ACP session");
                            let _ = event_tx.send(AcpEvent::Error(format!("Failed to create session: {}", e))).await;
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
                                // Notify that session ID changed
                                let _ = event_tx.send(AcpEvent::SessionChanged { new_session_id: new_id.clone() }).await;
                                (new_id, true)
                            }
                            Err(e2) => {
                                tracing::error!(error = %e2, "Failed to create fallback session");
                                let _ = event_tx.send(AcpEvent::Error(format!("Failed to create session: {}", e2))).await;
                                return Err(e2);
                            }
                        }
                    } else {
                        (existing_session_id.clone(), false)
                    }
                };

                // Send prompt and get event receiver
                let mut prompt_rx = match client.prompt(&active_session_id, &prompt_text).await {
                    Ok(rx) => rx,
                    Err(e) => {
                        tracing::error!(error = %e, "Failed to send prompt");
                        let _ = event_tx.send(AcpEvent::Error(format!("Failed to send prompt: {}", e))).await;
                        return Err(e);
                    }
                };

                // Forward events from prompt_rx to event_tx
                // Also track if we see a SessionChanged event
                let mut final_session_id = if session_changed { Some(active_session_id) } else { None };
                while let Some(event) = prompt_rx.recv().await {
                    // Track SessionChanged events
                    if let AcpEvent::SessionChanged { ref new_session_id } = event {
                        final_session_id = Some(new_session_id.clone());
                    }

                    if event_tx.send(event).await.is_err() {
                        break;
                    }
                }

                // Return the session ID if it changed
                Ok(final_session_id)
            }).await
        })
    })
    .await
    .context("Failed to spawn ACP blocking task")
    .and_then(|result| result)
    .map(|session_id| (event_rx, session_id))
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
