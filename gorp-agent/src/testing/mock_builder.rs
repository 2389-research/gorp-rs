// ABOUTME: Enhanced mock builder for sophisticated test scenarios.
// ABOUTME: Wraps MockBackend with tool sequence helpers, delays, streaming, and expectation verification.

use crate::backends::mock::MockBackend;
use crate::event::AgentEvent;
use crate::handle::AgentHandle;
use serde_json::Value;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Represents a tool call for use in test scenarios
#[derive(Debug, Clone)]
pub struct ToolCall {
    pub name: String,
    pub input: Value,
    pub output: Value,
    pub success: bool,
    pub duration_ms: u64,
}

/// Enhanced mock builder wrapping MockBackend with additional testing features
pub struct MockAgentBuilder {
    backend: MockBackend,
    expectations: Arc<Mutex<ExpectationState>>,
    current_builder: Option<EnhancedExpectationBuilder>,
}

#[derive(Default)]
struct ExpectationState {
    total_expectations: usize,
    consumed_expectations: usize,
    expected_prompt_count: Option<usize>,
    actual_prompt_count: usize,
}

struct EnhancedExpectationBuilder {
    pattern: String,
    delay: Option<Duration>,
    streaming_chunks: Vec<String>,
    events: Option<Vec<AgentEvent>>,
}

impl MockAgentBuilder {
    /// Create a new enhanced mock builder
    pub fn new() -> Self {
        Self {
            backend: MockBackend::new(),
            expectations: Arc::new(Mutex::new(ExpectationState::default())),
            current_builder: None,
        }
    }

    /// Set up an expectation for a prompt matching the given pattern
    pub fn on_prompt(mut self, pattern: &str) -> Self {
        // Finalize any pending builder
        if let Some(builder) = self.current_builder.take() {
            self = self.finalize_builder(builder);
        }

        // Create new builder
        self.current_builder = Some(EnhancedExpectationBuilder {
            pattern: pattern.to_string(),
            delay: None,
            streaming_chunks: Vec::new(),
            events: None,
        });

        // Increment total expectations
        self.expectations.lock().unwrap().total_expectations += 1;

        self
    }

    /// Add a delay before responding
    pub fn with_delay(mut self, duration: Duration) -> Self {
        if let Some(builder) = &mut self.current_builder {
            builder.delay = Some(duration);
        }
        self
    }

    /// Add streaming text chunks before the main response
    pub fn with_streaming(mut self, chunks: Vec<String>) -> Self {
        if let Some(builder) = &mut self.current_builder {
            builder.streaming_chunks = chunks;
        }
        self
    }

    /// Respond with a simple text result
    pub fn respond_text(mut self, text: &str) -> Self {
        if let Some(builder) = &mut self.current_builder {
            builder.events = Some(vec![AgentEvent::Result {
                text: text.to_string(),
                usage: None,
                metadata: serde_json::json!({}),
            }]);
        }
        self = self.finalize_current_builder();
        self
    }

    /// Respond with a sequence of tool calls followed by a result
    pub fn respond_with_tools(mut self, tools: Vec<ToolCall>, result: &str) -> Self {
        if let Some(builder) = &mut self.current_builder {
            let mut events = Vec::new();

            // Generate unique IDs for each tool
            for (idx, tool) in tools.iter().enumerate() {
                let tool_id = format!("tool-{}", idx + 1);

                // ToolStart event
                events.push(AgentEvent::ToolStart {
                    id: tool_id.clone(),
                    name: tool.name.clone(),
                    input: tool.input.clone(),
                });

                // ToolEnd event
                events.push(AgentEvent::ToolEnd {
                    id: tool_id,
                    name: tool.name.clone(),
                    output: tool.output.clone(),
                    success: tool.success,
                    duration_ms: tool.duration_ms,
                });
            }

            // Final result
            events.push(AgentEvent::Result {
                text: result.to_string(),
                usage: None,
                metadata: serde_json::json!({}),
            });

            builder.events = Some(events);
        }
        self = self.finalize_current_builder();
        self
    }

    /// Respond with custom events
    pub fn respond_with(mut self, events: Vec<AgentEvent>) -> Self {
        if let Some(builder) = &mut self.current_builder {
            builder.events = Some(events);
        }
        self = self.finalize_current_builder();
        self
    }

    /// Set the expected number of prompts to be sent
    pub fn expect_prompt_count(self, count: usize) -> Self {
        self.expectations.lock().unwrap().expected_prompt_count = Some(count);
        self
    }

    /// Verify that all expectations have been met
    pub fn verify_all_expectations_met(&self) -> Result<(), String> {
        let state = self.expectations.lock().unwrap();

        // Check if expected prompt count matches
        if let Some(expected) = state.expected_prompt_count {
            if state.actual_prompt_count != expected {
                return Err(format!(
                    "Expected {} prompts but received {}",
                    expected, state.actual_prompt_count
                ));
            }
        }

        // Check if all expectations were consumed
        if state.consumed_expectations < state.total_expectations {
            return Err(format!(
                "Not all expectations were consumed: {}/{} consumed",
                state.consumed_expectations, state.total_expectations
            ));
        }

        Ok(())
    }

    /// Convert this builder into an AgentHandle
    pub fn into_handle(mut self) -> EnhancedMockHandle {
        // Finalize any pending builder
        if let Some(builder) = self.current_builder.take() {
            self = self.finalize_builder(builder);
        }

        EnhancedMockHandle {
            backend: self.backend.into_handle(),
            expectations: self.expectations,
        }
    }

    /// Finalize the current builder and add to backend
    fn finalize_current_builder(mut self) -> Self {
        if let Some(builder) = self.current_builder.take() {
            self = self.finalize_builder(builder);
        }
        self
    }

    /// Finalize a builder and add its events to the backend
    fn finalize_builder(mut self, builder: EnhancedExpectationBuilder) -> Self {
        let mut events = Vec::new();

        // Add marker to track when this expectation is consumed
        events.push(AgentEvent::Custom {
            kind: "mock_expectation_consumed".to_string(),
            payload: serde_json::json!({}),
        });

        // Add streaming chunks
        for chunk in builder.streaming_chunks {
            events.push(AgentEvent::Text(chunk));
        }

        // Add main events
        if let Some(mut main_events) = builder.events {
            events.append(&mut main_events);
        }

        // Wrap with delay if specified
        if let Some(delay) = builder.delay {
            events = Self::wrap_with_delay(events, delay);
        }

        // Add to backend
        self.backend = self
            .backend
            .on_prompt(&builder.pattern)
            .respond_with(events);

        self
    }

    /// Wrap events with a delay
    fn wrap_with_delay(events: Vec<AgentEvent>, delay: Duration) -> Vec<AgentEvent> {
        // Insert a custom event at the start that signals a delay
        let mut result = vec![AgentEvent::Custom {
            kind: "mock_delay".to_string(),
            payload: serde_json::json!({ "duration_ms": delay.as_millis() as u64 }),
        }];
        result.extend(events);
        result
    }
}

impl Default for MockAgentBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Enhanced mock handle that wraps an AgentHandle with expectation verification and delay handling
pub struct EnhancedMockHandle {
    backend: AgentHandle,
    expectations: Arc<Mutex<ExpectationState>>,
}

impl EnhancedMockHandle {
    /// Create a new session
    pub async fn new_session(&self) -> anyhow::Result<String> {
        self.backend.new_session().await
    }

    /// Load an existing session
    pub async fn load_session(&self, session_id: &str) -> anyhow::Result<()> {
        self.backend.load_session(session_id).await
    }

    /// Send a prompt and receive events with delay handling
    pub async fn prompt(
        &self,
        session_id: &str,
        text: &str,
    ) -> anyhow::Result<crate::handle::EventReceiver> {
        // Track prompt count
        self.expectations.lock().unwrap().actual_prompt_count += 1;

        // Get the event receiver from backend
        let mut receiver = self.backend.prompt(session_id, text).await?;

        // Create a new channel for filtered events
        let (tx, rx) = tokio::sync::mpsc::channel(2048);

        // Clone expectations to track consumption
        let expectations = self.expectations.clone();

        // Spawn a task to filter delay events and apply delays
        tokio::spawn(async move {
            while let Some(event) = receiver.recv().await {
                // Check if this is a special mock event
                if let AgentEvent::Custom { kind, payload } = &event {
                    if kind == "mock_delay" {
                        if let Some(duration_ms) =
                            payload.get("duration_ms").and_then(|v| v.as_u64())
                        {
                            tokio::time::sleep(Duration::from_millis(duration_ms)).await;
                        }
                        continue; // Don't send the delay event to the client
                    } else if kind == "mock_expectation_consumed" {
                        // Track that an expectation was consumed
                        expectations.lock().unwrap().consumed_expectations += 1;
                        continue; // Don't send this marker to the client
                    }
                }

                // Send all other events
                if tx.send(event).await.is_err() {
                    break;
                }
            }
        });

        Ok(crate::handle::EventReceiver::new(rx))
    }

    /// Cancel an in-progress prompt
    pub async fn cancel(&self, session_id: &str) -> anyhow::Result<()> {
        self.backend.cancel(session_id).await
    }

    /// Get the backend name
    pub fn name(&self) -> &'static str {
        self.backend.name()
    }

    /// Verify all expectations were met
    pub fn verify_all_expectations_met(&self) -> Result<(), String> {
        let state = self.expectations.lock().unwrap();

        // Check if expected prompt count matches
        if let Some(expected) = state.expected_prompt_count {
            if state.actual_prompt_count != expected {
                return Err(format!(
                    "Expected {} prompts but received {}",
                    expected, state.actual_prompt_count
                ));
            }
        }

        // Check if all expectations were consumed
        if state.consumed_expectations < state.total_expectations {
            return Err(format!(
                "Not all expectations were consumed: {}/{} consumed",
                state.consumed_expectations, state.total_expectations
            ));
        }

        Ok(())
    }
}
