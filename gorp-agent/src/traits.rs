// ABOUTME: Core AgentBackend trait that all backends implement.
// ABOUTME: Defines session management and prompt execution interface.

use crate::AgentEvent;
use anyhow::Result;
use futures::future::BoxFuture;
use futures::stream::BoxStream;

/// Core trait that all agent backends implement.
///
/// Backends may have `!Send` internals (like ACP), but the trait methods
/// return boxed futures that can be sent to other threads via AgentHandle.
pub trait AgentBackend {
    /// Backend name for logging and metrics
    fn name(&self) -> &'static str;

    /// Create a new session, returns the session ID
    fn new_session<'a>(&'a self) -> BoxFuture<'a, Result<String>>;

    /// Load/resume an existing session by ID
    fn load_session<'a>(&'a self, session_id: &'a str) -> BoxFuture<'a, Result<()>>;

    /// Send a prompt and receive a stream of events
    ///
    /// The returned stream emits events as they occur (text chunks, tool calls,
    /// etc.) and completes with a Result or Error event.
    fn prompt<'a>(
        &'a self,
        session_id: &'a str,
        text: &'a str,
    ) -> BoxFuture<'a, Result<BoxStream<'a, AgentEvent>>>;

    /// Cancel an in-progress prompt
    fn cancel<'a>(&'a self, session_id: &'a str) -> BoxFuture<'a, Result<()>>;
}
