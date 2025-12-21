// ABOUTME: Mock backend for testing - returns pre-configured responses.
// ABOUTME: Allows deterministic tests without spawning real agent processes.
//!
//! # Example
//!
//! ```no_run
//! use gorp_agent::backends::mock::MockBackend;
//! use gorp_agent::AgentEvent;
//! use serde_json::json;
//!
//! # async fn example() {
//! let mock = MockBackend::new()
//!     .on_prompt("hello").respond_text("Hi there!")
//!     .on_prompt("read file").respond_with(vec![
//!         AgentEvent::ToolStart {
//!             id: "t1".to_string(),
//!             name: "Read".to_string(),
//!             input: json!({"path": "/tmp/foo"}),
//!         },
//!         AgentEvent::ToolEnd {
//!             id: "t1".to_string(),
//!             name: "Read".to_string(),
//!             output: json!({"content": "file contents"}),
//!             success: true,
//!             duration_ms: 10,
//!         },
//!         AgentEvent::Result {
//!             text: "Read the file".to_string(),
//!             usage: None,
//!             metadata: json!({}),
//!         },
//!     ]);
//!
//! let handle = mock.into_handle();
//! let session_id = handle.new_session().await.unwrap();
//! let mut receiver = handle.prompt(&session_id, "hello").await.unwrap();
//!
//! if let Some(AgentEvent::Result { text, .. }) = receiver.recv().await {
//!     assert_eq!(text, "Hi there!");
//! }
//! # }
//! ```

use crate::event::AgentEvent;
use crate::handle::{AgentHandle, Command};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

/// Mock backend for testing
pub struct MockBackend {
    expectations: Arc<Mutex<VecDeque<Expectation>>>,
}

struct Expectation {
    pattern: String,
    events: Vec<AgentEvent>,
}

impl MockBackend {
    /// Create a new mock backend with no expectations
    pub fn new() -> Self {
        Self {
            expectations: Arc::new(Mutex::new(VecDeque::new())),
        }
    }

    /// Set up an expectation for a prompt matching the given pattern
    pub fn on_prompt(self, pattern: &str) -> ExpectationBuilder {
        ExpectationBuilder {
            backend: self,
            pattern: pattern.to_string(),
        }
    }

    /// Convert this backend into an AgentHandle
    pub fn into_handle(self) -> AgentHandle {
        let (tx, mut rx) = mpsc::channel::<Command>(32);
        let name = "mock";
        let expectations = self.expectations;

        tokio::spawn(async move {
            let mut session_counter = 0u64;

            while let Some(cmd) = rx.recv().await {
                match cmd {
                    Command::NewSession { reply } => {
                        session_counter += 1;
                        let _ = reply.send(Ok(format!("mock-session-{}", session_counter)));
                    }
                    Command::LoadSession { reply, .. } => {
                        let _ = reply.send(Ok(()));
                    }
                    Command::Prompt {
                        text,
                        event_tx,
                        reply,
                        .. // session_id and is_new_session not used by mock backend
                    } => {
                        let _ = reply.send(Ok(()));

                        // Match expectations with FIFO preference: check the front first,
                        // fall back to searching the queue if front doesn't match.
                        // This allows deterministic ordering when prompts arrive in order,
                        // while still finding matches for out-of-order prompts.
                        let events = {
                            let mut exp = expectations.lock().unwrap_or_else(|e| e.into_inner());
                            if let Some(front) = exp.front() {
                                if text.contains(&front.pattern) {
                                    exp.pop_front().map(|e| e.events)
                                } else {
                                    // If front doesn't match, search for first matching one
                                    exp.iter()
                                        .position(|e| text.contains(&e.pattern))
                                        .and_then(|i| exp.remove(i))
                                        .map(|e| e.events)
                                }
                            } else {
                                None
                            }
                        };

                        if let Some(events) = events {
                            for event in events {
                                if event_tx.send(event).await.is_err() {
                                    break;
                                }
                            }
                        } else {
                            let _ = event_tx
                                .send(AgentEvent::Result {
                                    text: format!("Mock: no expectation for '{}'", text),
                                    usage: None,
                                    metadata: serde_json::json!({}),
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

    /// Factory function for the registry
    pub fn factory() -> crate::registry::BackendFactory {
        Box::new(|_config| {
            let backend = MockBackend::new();
            Ok(backend.into_handle())
        })
    }
}

impl Default for MockBackend {
    fn default() -> Self {
        Self::new()
    }
}

/// Builder for setting up mock expectations with a fluent API
pub struct ExpectationBuilder {
    backend: MockBackend,
    pattern: String,
}

impl ExpectationBuilder {
    /// Respond with a list of events
    pub fn respond_with(self, events: Vec<AgentEvent>) -> MockBackend {
        self.backend
            .expectations
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push_back(Expectation {
                pattern: self.pattern,
                events,
            });
        self.backend
    }

    /// Respond with a simple text result
    pub fn respond_text(self, text: &str) -> MockBackend {
        self.respond_with(vec![AgentEvent::Result {
            text: text.to_string(),
            usage: None,
            metadata: serde_json::json!({}),
        }])
    }

    /// Respond with an error
    pub fn respond_error(self, code: crate::event::ErrorCode, message: &str) -> MockBackend {
        self.respond_with(vec![AgentEvent::Error {
            code,
            message: message.to_string(),
            recoverable: false,
        }])
    }
}
