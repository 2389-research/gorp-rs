// ABOUTME: Gateway adapter abstraction for platform-agnostic message routing.
// ABOUTME: Defines the GatewayAdapter trait that all platform integrations implement.

pub mod registry;
pub mod web;

use async_trait::async_trait;
use std::sync::Arc;

use crate::bus::{MessageBus, ResponseContent};

/// Trait for platform gateway adapters. Each adapter translates between
/// platform-native events and bus types. Adapters have two loops:
/// inbound (platform -> bus) and outbound (bus -> platform).
#[async_trait]
pub trait GatewayAdapter: Send + Sync {
    /// Unique platform identifier (e.g., "matrix", "slack", "telegram", "web")
    fn platform_id(&self) -> &str;

    /// Start the adapter's inbound and outbound loops.
    async fn start(&self, bus: Arc<MessageBus>) -> anyhow::Result<()>;

    /// Send a response to a specific channel on this platform.
    async fn send(&self, channel_id: &str, content: ResponseContent) -> anyhow::Result<()>;

    /// Graceful shutdown.
    async fn stop(&self) -> anyhow::Result<()>;
}
