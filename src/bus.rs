// ABOUTME: Core message bus types and pub/sub infrastructure for platform-agnostic message routing.
// ABOUTME: Defines BusMessage, BusResponse, MessageBus (broadcast channels + channel bindings).

use std::collections::HashMap;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use tokio::sync::{broadcast, RwLock};

/// A message entering the bus from any source (gateway, web, API).
#[derive(Debug, Clone)]
pub struct BusMessage {
    /// Unique event ID for deduplication
    pub id: String,
    /// Which gateway/interface sent this message
    pub source: MessageSource,
    /// Where the message should be routed
    pub session_target: SessionTarget,
    /// Human-readable sender identity
    pub sender: String,
    /// Message content
    pub body: String,
    /// When the message was created
    pub timestamp: DateTime<Utc>,
}

/// Identifies where a message originated.
#[derive(Debug, Clone)]
pub enum MessageSource {
    /// From a chat platform (Matrix, Slack, Telegram)
    Platform {
        platform_id: String,
        channel_id: String,
    },
    /// From the web admin chat UI
    Web {
        connection_id: String,
    },
    /// From the webhook API
    Api {
        token_hint: String,
    },
}

/// Where a message should be routed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionTarget {
    /// Unmapped channel — route to DISPATCH command handler
    Dispatch,
    /// Mapped channel — route to the named agent session
    Session { name: String },
}

/// A response leaving the bus toward connected platforms.
#[derive(Debug, Clone)]
pub struct BusResponse {
    /// Which agent session produced this response
    pub session_name: String,
    /// Response payload
    pub content: ResponseContent,
    /// When the response was generated
    pub timestamp: DateTime<Utc>,
}

/// Payload types for outbound responses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResponseContent {
    /// Streaming text chunk (partial output)
    Chunk(String),
    /// Final assembled response
    Complete(String),
    /// Agent or system error
    Error(String),
    /// DISPATCH or system notification
    SystemNotice(String),
}

/// Pub/sub message bus with broadcast channels and channel-to-session bindings.
///
/// Uses tokio broadcast channels for fan-out delivery of inbound messages and
/// outbound responses. Maintains an in-memory map from (platform_id, channel_id)
/// pairs to session names for routing decisions.
#[derive(Clone)]
pub struct MessageBus {
    inbound_tx: broadcast::Sender<BusMessage>,
    outbound_tx: broadcast::Sender<BusResponse>,
    channel_map: Arc<RwLock<HashMap<(String, String), String>>>,
}

impl MessageBus {
    /// Create a new MessageBus with the given broadcast channel capacity.
    pub fn new(capacity: usize) -> Self {
        let (inbound_tx, _) = broadcast::channel(capacity);
        let (outbound_tx, _) = broadcast::channel(capacity);
        Self {
            inbound_tx,
            outbound_tx,
            channel_map: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Publish an inbound message to all subscribers.
    pub fn publish_inbound(&self, msg: BusMessage) {
        // Ignore send errors (no active receivers is not an error condition)
        let _ = self.inbound_tx.send(msg);
    }

    /// Subscribe to inbound messages.
    pub fn subscribe_inbound(&self) -> broadcast::Receiver<BusMessage> {
        self.inbound_tx.subscribe()
    }

    /// Publish an outbound response to all subscribers.
    pub fn publish_response(&self, resp: BusResponse) {
        let _ = self.outbound_tx.send(resp);
    }

    /// Subscribe to outbound responses.
    pub fn subscribe_responses(&self) -> broadcast::Receiver<BusResponse> {
        self.outbound_tx.subscribe()
    }

    /// Resolve (platform_id, channel_id) to a SessionTarget (sync, blocking).
    ///
    /// Returns `SessionTarget::Session` if a binding exists, `SessionTarget::Dispatch` otherwise.
    /// Uses `blocking_read()` — do not call from within an async runtime task.
    pub fn resolve_target(&self, platform_id: &str, channel_id: &str) -> SessionTarget {
        let map = self.channel_map.blocking_read();
        match map.get(&(platform_id.to_string(), channel_id.to_string())) {
            Some(session_name) => SessionTarget::Session {
                name: session_name.clone(),
            },
            None => SessionTarget::Dispatch,
        }
    }

    /// Resolve (platform_id, channel_id) to a SessionTarget (async).
    ///
    /// Returns `SessionTarget::Session` if a binding exists, `SessionTarget::Dispatch` otherwise.
    pub async fn resolve_target_async(
        &self,
        platform_id: &str,
        channel_id: &str,
    ) -> SessionTarget {
        let map = self.channel_map.read().await;
        match map.get(&(platform_id.to_string(), channel_id.to_string())) {
            Some(session_name) => SessionTarget::Session {
                name: session_name.clone(),
            },
            None => SessionTarget::Dispatch,
        }
    }

    /// Bind a (platform_id, channel_id) pair to a session name (sync, blocking).
    ///
    /// Uses `blocking_write()` — do not call from within an async runtime task.
    pub fn bind_channel(&self, platform_id: &str, channel_id: &str, session_name: &str) {
        let mut map = self.channel_map.blocking_write();
        map.insert(
            (platform_id.to_string(), channel_id.to_string()),
            session_name.to_string(),
        );
    }

    /// Bind a (platform_id, channel_id) pair to a session name (async).
    pub async fn bind_channel_async(
        &self,
        platform_id: &str,
        channel_id: &str,
        session_name: &str,
    ) {
        let mut map = self.channel_map.write().await;
        map.insert(
            (platform_id.to_string(), channel_id.to_string()),
            session_name.to_string(),
        );
    }

    /// Remove the binding for a (platform_id, channel_id) pair (sync, blocking).
    ///
    /// Uses `blocking_write()` — do not call from within an async runtime task.
    pub fn unbind_channel(&self, platform_id: &str, channel_id: &str) {
        let mut map = self.channel_map.blocking_write();
        map.remove(&(platform_id.to_string(), channel_id.to_string()));
    }

    /// Remove the binding for a (platform_id, channel_id) pair (async).
    pub async fn unbind_channel_async(&self, platform_id: &str, channel_id: &str) {
        let mut map = self.channel_map.write().await;
        map.remove(&(platform_id.to_string(), channel_id.to_string()));
    }

    /// List all (platform_id, channel_id) pairs bound to a given session (sync, blocking).
    ///
    /// Uses `blocking_read()` — do not call from within an async runtime task.
    pub fn bindings_for_session(&self, session_name: &str) -> Vec<(String, String)> {
        let map = self.channel_map.blocking_read();
        map.iter()
            .filter(|(_, v)| v.as_str() == session_name)
            .map(|((p, c), _)| (p.clone(), c.clone()))
            .collect()
    }

    /// List all (platform_id, channel_id) pairs bound to a given session (async).
    pub async fn bindings_for_session_async(
        &self,
        session_name: &str,
    ) -> Vec<(String, String)> {
        let map = self.channel_map.read().await;
        map.iter()
            .filter(|(_, v)| v.as_str() == session_name)
            .map(|((p, c), _)| (p.clone(), c.clone()))
            .collect()
    }

    /// Bulk-load bindings from a list of (platform_id, channel_id, session_name) triples.
    pub async fn load_bindings(&self, bindings: Vec<(String, String, String)>) {
        let mut map = self.channel_map.write().await;
        for (platform_id, channel_id, session_name) in bindings {
            map.insert((platform_id, channel_id), session_name);
        }
    }
}
