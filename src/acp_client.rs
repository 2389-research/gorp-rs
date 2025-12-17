// ABOUTME: This module provides an ACP (Agent Client Protocol) client for communicating with AI agents.
// ABOUTME: It replaces direct Claude CLI spawning with the standardized ACP protocol over stdio.

use agent_client_protocol as acp;
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::process::{Child, Command};
use tokio::sync::{mpsc, Mutex};
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

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

/// Handler for ACP client-side callbacks
#[derive(Clone)]
struct AcpClientHandler {
    event_tx: Arc<Mutex<Option<mpsc::Sender<AcpEvent>>>>,
}

impl AcpClientHandler {
    fn new() -> Self {
        Self {
            event_tx: Arc::new(Mutex::new(None)),
        }
    }

    fn set_event_sender(&self, tx: mpsc::Sender<AcpEvent>) {
        if let Ok(mut guard) = self.event_tx.try_lock() {
            *guard = Some(tx);
        }
    }

    async fn send_event(&self, event: AcpEvent) {
        if let Some(tx) = self.event_tx.lock().await.as_ref() {
            let _ = tx.send(event).await;
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
                acp::RequestPermissionOutcome::Selected(
                    acp::SelectedPermissionOutcome::new(option.option_id.clone())
                )
            ))
        } else {
            // No options available, return cancelled
            Ok(acp::RequestPermissionResponse::new(
                acp::RequestPermissionOutcome::Cancelled
            ))
        }
    }

    async fn session_notification(
        &self,
        args: acp::SessionNotification,
    ) -> acp::Result<()> {
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
                    self.send_event(AcpEvent::Text(text)).await;
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
                self.send_event(AcpEvent::ToolUse {
                    name,
                    input_preview: preview,
                })
                .await;
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
    _child: Child,
    conn: acp::ClientSideConnection,
    handler: Arc<AcpClientHandler>,
    working_dir: PathBuf,
}

impl AcpClient {
    /// Spawn a new agent process and establish ACP connection
    pub async fn spawn(working_dir: &Path, agent_binary: &str) -> Result<Self> {
        // Validate inputs
        if agent_binary.contains("..") || agent_binary.contains('\0') {
            anyhow::bail!("Invalid agent binary path");
        }
        if !working_dir.exists() {
            anyhow::bail!("Working directory does not exist: {}", working_dir.display());
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

        let handler = Arc::new(AcpClientHandler::new());
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
            _child: child,
            conn,
            handler,
            working_dir: working_dir.to_path_buf(),
        })
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
