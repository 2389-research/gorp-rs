// ABOUTME: Recording and replay infrastructure for transcript-based testing.
// ABOUTME: Enables capturing interactions for test replay and deterministic testing.

use crate::event::AgentEvent;
use crate::handle::{AgentHandle, Command, EventReceiver};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::path::Path;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

/// An interaction recorded during a session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Interaction {
    pub timestamp: std::time::SystemTime,
    pub session_id: String,
    pub prompt: String,
    pub events: Vec<AgentEvent>,
}

/// Records all interactions for later replay
pub struct RecordingAgent {
    inner: AgentHandle,
    transcript: Arc<Mutex<Vec<Interaction>>>,
}

impl RecordingAgent {
    /// Wrap an AgentHandle to record all interactions
    pub fn wrap(inner: AgentHandle) -> Self {
        Self {
            inner,
            transcript: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Decompose into the inner handle and recorded transcript
    pub fn into_parts(self) -> (AgentHandle, Vec<Interaction>) {
        let transcript = match Arc::try_unwrap(self.transcript) {
            Ok(mutex) => mutex.into_inner().unwrap(),
            Err(arc) => arc.lock().unwrap().clone(),
        };
        (self.inner, transcript)
    }

    /// Get a copy of the current transcript
    pub fn transcript(&self) -> Vec<Interaction> {
        self.transcript.lock().unwrap().clone()
    }

    /// Save transcript to a file
    pub async fn save_transcript(&self, path: &Path) -> Result<()> {
        let transcript = self.transcript();
        let json = serde_json::to_string_pretty(&transcript)?;
        tokio::fs::write(path, json).await?;
        Ok(())
    }

    /// Create a new session
    pub async fn new_session(&self) -> Result<String> {
        self.inner.new_session().await
    }

    /// Load an existing session
    pub async fn load_session(&self, session_id: &str) -> Result<()> {
        self.inner.load_session(session_id).await
    }

    /// Send a prompt and receive events, recording the interaction
    pub async fn prompt(&self, session_id: &str, text: &str) -> Result<EventReceiver> {
        let mut receiver = self.inner.prompt(session_id, text).await?;

        // Create a new channel to intercept events
        let (event_tx, event_rx) = mpsc::channel(2048);

        // Record the interaction
        let transcript = Arc::clone(&self.transcript);
        let session_id = session_id.to_string();
        let prompt = text.to_string();

        tokio::spawn(async move {
            let mut events = Vec::new();

            // Forward events and collect them
            while let Some(event) = receiver.recv().await {
                events.push(event.clone());
                if event_tx.send(event).await.is_err() {
                    break;
                }
            }

            // Record the interaction
            transcript.lock().unwrap().push(Interaction {
                timestamp: std::time::SystemTime::now(),
                session_id,
                prompt,
                events,
            });
        });

        Ok(EventReceiver::new(event_rx))
    }

    /// Cancel an in-progress prompt
    pub async fn cancel(&self, session_id: &str) -> Result<()> {
        self.inner.cancel(session_id).await
    }
}

/// Replays recorded interactions
pub struct ReplayAgent {
    transcript: Arc<Mutex<VecDeque<Interaction>>>,
}

impl ReplayAgent {
    /// Create a replay agent from a transcript
    pub fn from_transcript(transcript: Vec<Interaction>) -> Self {
        Self {
            transcript: Arc::new(Mutex::new(transcript.into())),
        }
    }

    /// Load a transcript from a file
    pub async fn load(path: &Path) -> Result<Self> {
        let json = tokio::fs::read_to_string(path).await?;
        let transcript: Vec<Interaction> = serde_json::from_str(&json)?;
        Ok(Self::from_transcript(transcript))
    }

    /// Convert into an AgentHandle that replays the transcript
    pub fn into_handle(self) -> AgentHandle {
        let (tx, mut rx) = mpsc::channel::<Command>(32);
        let name = "replay";
        let transcript = self.transcript;

        tokio::spawn(async move {
            let mut session_counter = 0u64;

            while let Some(cmd) = rx.recv().await {
                match cmd {
                    Command::NewSession { reply } => {
                        session_counter += 1;
                        let _ = reply.send(Ok(format!("replay-session-{}", session_counter)));
                    }
                    Command::LoadSession { reply, .. } => {
                        let _ = reply.send(Ok(()));
                    }
                    Command::Prompt {
                        text,
                        event_tx,
                        reply,
                        ..
                    } => {
                        let _ = reply.send(Ok(()));

                        // Find matching interaction in transcript
                        let interaction = {
                            let mut t = transcript.lock().unwrap();
                            t.iter()
                                .position(|i| i.prompt == text)
                                .and_then(|idx| Some(t.remove(idx).unwrap()))
                        };

                        if let Some(interaction) = interaction {
                            // Replay the recorded events
                            for event in interaction.events {
                                if event_tx.send(event).await.is_err() {
                                    break;
                                }
                            }
                        } else {
                            // No matching interaction found
                            let _ = event_tx
                                .send(AgentEvent::Error {
                                    code: crate::event::ErrorCode::Unknown,
                                    message: format!(
                                        "No recorded interaction for prompt: {}",
                                        text
                                    ),
                                    recoverable: false,
                                })
                                .await;
                        }
                    }
                    Command::Cancel { reply, .. } => {
                        let _ = reply.send(Ok(()));
                    }
                }
            }
        });

        AgentHandle::new(tx, name)
    }
}
