// ABOUTME: Core message bus types for platform-agnostic message routing.
// ABOUTME: Defines BusMessage (inbound), BusResponse (outbound), and supporting enums.

use chrono::{DateTime, Utc};

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
