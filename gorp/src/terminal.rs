// ABOUTME: Terminal PTY management for spawning shells in containers.
// ABOUTME: Handles PTY creation, I/O streaming, and session lifecycle.

use anyhow::{Context, Result};
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex, RwLock};
use uuid::Uuid;

/// Terminal session state
pub struct TerminalSession {
    pub id: String,
    pub workspace_path: String,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    shutdown_tx: Option<mpsc::Sender<()>>,
}

impl TerminalSession {
    /// Write data to the PTY (user input)
    pub async fn write(&self, data: &[u8]) -> Result<()> {
        let mut writer = self.writer.lock().await;
        writer.write_all(data).context("Failed to write to PTY")?;
        writer.flush().context("Failed to flush PTY")?;
        Ok(())
    }

    /// Signal shutdown
    pub async fn shutdown(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(()).await;
        }
    }
}

/// Manages terminal sessions
pub struct TerminalManager {
    sessions: RwLock<HashMap<String, Arc<TerminalSession>>>,
}

impl TerminalManager {
    pub fn new() -> Self {
        Self {
            sessions: RwLock::new(HashMap::new()),
        }
    }

    /// Spawn a new terminal session
    pub async fn spawn(
        &self,
        workspace_path: String,
        output_tx: mpsc::Sender<Vec<u8>>,
    ) -> Result<Arc<TerminalSession>> {
        let session_id = Uuid::new_v4().to_string();

        // Create PTY
        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize {
                rows: 24,
                cols: 80,
                pixel_width: 0,
                pixel_height: 0,
            })
            .context("Failed to open PTY")?;

        // Spawn shell
        let mut cmd = CommandBuilder::new("bash");
        cmd.cwd(&workspace_path);
        cmd.env("TERM", "xterm-256color");

        let _child = pair
            .slave
            .spawn_command(cmd)
            .context("Failed to spawn shell")?;

        // Get reader and writer
        let reader = pair
            .master
            .try_clone_reader()
            .context("Failed to clone PTY reader")?;
        let writer = pair
            .master
            .take_writer()
            .context("Failed to take PTY writer")?;

        // Create shutdown channel
        let (shutdown_tx, mut shutdown_rx) = mpsc::channel::<()>(1);

        // Spawn reader task
        let session_id_clone = session_id.clone();
        tokio::task::spawn_blocking(move || {
            let mut reader = reader;
            let mut buffer = [0u8; 4096];

            loop {
                // Check for shutdown (non-blocking)
                if shutdown_rx.try_recv().is_ok() {
                    break;
                }

                match reader.read(&mut buffer) {
                    Ok(0) => break, // EOF
                    Ok(n) => {
                        let data = buffer[..n].to_vec();
                        if output_tx.blocking_send(data).is_err() {
                            break; // Channel closed
                        }
                    }
                    Err(e) => {
                        tracing::error!(session = %session_id_clone, error = %e, "PTY read error");
                        break;
                    }
                }
            }
            tracing::info!(session = %session_id_clone, "PTY reader stopped");
        });

        let session = Arc::new(TerminalSession {
            id: session_id.clone(),
            workspace_path,
            writer: Arc::new(Mutex::new(writer)),
            shutdown_tx: Some(shutdown_tx),
        });

        self.sessions.write().await.insert(session_id, session.clone());

        Ok(session)
    }

    /// Get an existing session
    pub async fn get(&self, session_id: &str) -> Option<Arc<TerminalSession>> {
        self.sessions.read().await.get(session_id).cloned()
    }

    /// Remove a session
    pub async fn remove(&self, session_id: &str) -> Option<Arc<TerminalSession>> {
        self.sessions.write().await.remove(session_id)
    }

    /// Resize a terminal
    pub async fn resize(&self, session_id: &str, rows: u16, cols: u16) -> Result<()> {
        // Note: portable-pty resize requires access to the master, which we don't store
        // For now, log and skip - can be enhanced later
        tracing::debug!(session = %session_id, rows, cols, "Terminal resize requested (not implemented)");
        Ok(())
    }
}

impl Default for TerminalManager {
    fn default() -> Self {
        Self::new()
    }
}
