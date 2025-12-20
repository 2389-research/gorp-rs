// ABOUTME: AgentHandle provides Send+Sync wrapper around potentially !Send backends.
// ABOUTME: Uses channels to communicate with backend worker thread.

use crate::AgentEvent;
use anyhow::Result;
use tokio::sync::{mpsc, oneshot};

/// Commands sent from AgentHandle to the backend worker
#[derive(Debug)]
pub enum Command {
    NewSession {
        reply: oneshot::Sender<Result<String>>,
    },
    LoadSession {
        session_id: String,
        reply: oneshot::Sender<Result<()>>,
    },
    Prompt {
        session_id: String,
        text: String,
        event_tx: mpsc::Sender<AgentEvent>,
        reply: oneshot::Sender<Result<()>>,
    },
    Cancel {
        session_id: String,
        reply: oneshot::Sender<Result<()>>,
    },
}

/// Send + Sync handle that gorp interacts with.
///
/// Internally communicates with a worker thread/task that runs the actual
/// backend. This allows backends with `!Send` futures (like ACP) to be
/// used safely across async tasks.
#[derive(Clone)]
pub struct AgentHandle {
    tx: mpsc::Sender<Command>,
    name: &'static str,
}

impl AgentHandle {
    /// Create a new AgentHandle with the given command channel and backend name
    pub fn new(tx: mpsc::Sender<Command>, name: &'static str) -> Self {
        Self { tx, name }
    }

    /// Get the backend name
    pub fn name(&self) -> &'static str {
        self.name
    }

    /// Create a new session
    pub async fn new_session(&self) -> Result<String> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.tx
            .send(Command::NewSession { reply: reply_tx })
            .await
            .map_err(|_| anyhow::anyhow!("Backend worker closed"))?;
        reply_rx
            .await
            .map_err(|_| anyhow::anyhow!("Backend worker dropped reply channel"))?
    }

    /// Load an existing session
    pub async fn load_session(&self, session_id: &str) -> Result<()> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.tx
            .send(Command::LoadSession {
                session_id: session_id.to_string(),
                reply: reply_tx,
            })
            .await
            .map_err(|_| anyhow::anyhow!("Backend worker closed"))?;
        reply_rx
            .await
            .map_err(|_| anyhow::anyhow!("Backend worker dropped reply channel"))?
    }

    /// Send a prompt and receive events via EventReceiver
    pub async fn prompt(&self, session_id: &str, text: &str) -> Result<EventReceiver> {
        let (event_tx, event_rx) = mpsc::channel(2048);
        let (reply_tx, reply_rx) = oneshot::channel();

        self.tx
            .send(Command::Prompt {
                session_id: session_id.to_string(),
                text: text.to_string(),
                event_tx,
                reply: reply_tx,
            })
            .await
            .map_err(|_| anyhow::anyhow!("Backend worker closed"))?;

        // Wait for the backend to acknowledge the prompt started
        reply_rx
            .await
            .map_err(|_| anyhow::anyhow!("Backend worker dropped reply channel"))??;

        Ok(EventReceiver::new(event_rx))
    }

    /// Cancel an in-progress prompt
    pub async fn cancel(&self, session_id: &str) -> Result<()> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.tx
            .send(Command::Cancel {
                session_id: session_id.to_string(),
                reply: reply_tx,
            })
            .await
            .map_err(|_| anyhow::anyhow!("Backend worker closed"))?;
        reply_rx
            .await
            .map_err(|_| anyhow::anyhow!("Backend worker dropped reply channel"))?
    }
}

/// Receiver for streaming events from a prompt.
///
/// This is `Send` so it can be passed across async task boundaries.
pub struct EventReceiver {
    rx: mpsc::Receiver<AgentEvent>,
}

impl EventReceiver {
    /// Create a new EventReceiver wrapping the given channel
    pub fn new(rx: mpsc::Receiver<AgentEvent>) -> Self {
        Self { rx }
    }

    /// Receive the next event, or None if the stream is closed
    pub async fn recv(&mut self) -> Option<AgentEvent> {
        self.rx.recv().await
    }

    /// Try to receive an event without blocking
    pub fn try_recv(&mut self) -> Option<AgentEvent> {
        self.rx.try_recv().ok()
    }
}
