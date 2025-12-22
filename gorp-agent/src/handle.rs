// ABOUTME: AgentHandle provides Send+Sync wrapper around potentially !Send backends.
// ABOUTME: Uses channels to communicate with backend worker thread.

use crate::AgentEvent;
use anyhow::Result;
use tokio::sync::{mpsc, oneshot};

/// State of a session for tracking whether it needs initialization
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionState {
    /// Session was created via new_session() and no prompt has been sent yet.
    /// The first prompt should initialize this session on the backend.
    New,
    /// First prompt is currently being processed. Subsequent prompts should
    /// wait or treat this as an active session.
    FirstPromptInFlight,
    /// Session is active - first prompt has been processed.
    Active,
}

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
        /// True if this is the first prompt for a new session (needs backend initialization).
        /// False if the session was loaded or already has had a prompt.
        is_new_session: bool,
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
    /// Track session state to determine if a prompt needs to initialize the backend.
    /// Sessions created via new_session() start as New, transition to FirstPromptInFlight
    /// during the first prompt, then become Active. Sessions loaded via load_session()
    /// are not tracked here (treated as Active implicitly).
    session_states:
        std::sync::Arc<std::sync::RwLock<std::collections::HashMap<String, SessionState>>>,
}

impl AgentHandle {
    /// Create a new AgentHandle with the given command channel and backend name
    pub fn new(tx: mpsc::Sender<Command>, name: &'static str) -> Self {
        Self {
            tx,
            name,
            session_states: std::sync::Arc::new(std::sync::RwLock::new(
                std::collections::HashMap::new(),
            )),
        }
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
        let session_id = reply_rx
            .await
            .map_err(|_| anyhow::anyhow!("Backend worker dropped reply channel"))??;

        // Track that this session is new and needs initialization on first prompt.
        self.session_states
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .insert(session_id.clone(), SessionState::New);
        Ok(session_id)
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

        // Determine if this is a new session that needs initialization.
        // Use a state machine to handle concurrent prompts correctly:
        // - New -> FirstPromptInFlight: This prompt initializes the session
        // - FirstPromptInFlight -> FirstPromptInFlight: This prompt waits (is_new_session=false)
        // - Active -> Active: Session already initialized (is_new_session=false)
        // - Not tracked (loaded session) -> is_new_session=false
        let is_new_session = {
            let mut states = self
                .session_states
                .write()
                .unwrap_or_else(|e| e.into_inner());
            match states.get(session_id) {
                Some(SessionState::New) => {
                    // First prompt for this new session - we'll initialize it
                    states.insert(session_id.to_string(), SessionState::FirstPromptInFlight);
                    true
                }
                Some(SessionState::FirstPromptInFlight) | Some(SessionState::Active) => {
                    // Session is already being initialized or is active
                    false
                }
                None => {
                    // Session was loaded via load_session() or is unknown
                    false
                }
            }
        };

        self.tx
            .send(Command::Prompt {
                session_id: session_id.to_string(),
                text: text.to_string(),
                event_tx,
                reply: reply_tx,
                is_new_session,
            })
            .await
            .map_err(|_| anyhow::anyhow!("Backend worker closed"))?;

        // Wait for the backend to acknowledge the prompt started
        reply_rx
            .await
            .map_err(|_| anyhow::anyhow!("Backend worker dropped reply channel"))??;

        // If this was the first prompt, transition to Active state and clean up.
        // This prevents memory leaks from accumulating session state.
        if is_new_session {
            let mut states = self
                .session_states
                .write()
                .unwrap_or_else(|e| e.into_inner());
            // Transition to Active, then remove to prevent memory leaks.
            // We don't need to track active sessions - they're the default.
            states.remove(session_id);
        }

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

    /// Abandon a session that was created but will never be used.
    ///
    /// Call this if you create a session via `new_session()` but decide not to
    /// send any prompts to it. This cleans up internal tracking state to prevent
    /// memory leaks. Calling this on a session that has already received a prompt
    /// or was never created is safe (no-op).
    pub fn abandon_session(&self, session_id: &str) {
        self.session_states
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .remove(session_id);
    }

    /// Get the number of sessions currently being tracked.
    ///
    /// This is primarily useful for testing and debugging. Sessions are tracked
    /// from `new_session()` until their first `prompt()` or `abandon_session()`.
    pub fn tracked_session_count(&self) -> usize {
        self.session_states
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .len()
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
        match self.rx.try_recv() {
            Ok(event) => Some(event),
            Err(tokio::sync::mpsc::error::TryRecvError::Empty) => None,
            Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => {
                tracing::debug!("Event channel disconnected");
                None
            }
        }
    }
}
