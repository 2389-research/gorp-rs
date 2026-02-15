// ABOUTME: Platform registry that manages multiple chat platform instances
// ABOUTME: Merges event streams, handles shutdown, and aggregates health status

use anyhow::Result;
use futures_util::stream::SelectAll;
use gorp_core::{EventStream, IncomingMessage, MessagingPlatform, PlatformConnectionState};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

/// Thread-safe shared registry for use across admin, websocket, and main tasks
pub type SharedPlatformRegistry = Arc<tokio::sync::RwLock<PlatformRegistry>>;

/// Health status for a single platform
#[derive(Debug, Clone)]
pub struct PlatformHealth {
    pub platform_id: String,
    pub state: PlatformConnectionState,
}

/// Registry of all active chat platforms.
/// Holds platform instances, merges their event streams, and coordinates lifecycle.
pub struct PlatformRegistry {
    platforms: HashMap<String, Box<dyn MessagingPlatform>>,
}

impl PlatformRegistry {
    pub fn new() -> Self {
        Self {
            platforms: HashMap::new(),
        }
    }

    /// Register a platform. Uses platform_id() as the key.
    pub fn register(&mut self, platform: Box<dyn MessagingPlatform>) {
        let id = platform.platform_id().to_string();
        self.platforms.insert(id, platform);
    }

    /// Get a platform by its ID.
    pub fn get(&self, platform_id: &str) -> Option<&dyn MessagingPlatform> {
        self.platforms.get(platform_id).map(|p| p.as_ref())
    }

    /// Check if any platforms are registered.
    pub fn is_empty(&self) -> bool {
        self.platforms.is_empty()
    }

    /// Number of registered platforms.
    pub fn len(&self) -> usize {
        self.platforms.len()
    }

    /// Get all registered platform IDs.
    pub fn platform_ids(&self) -> Vec<String> {
        self.platforms.keys().cloned().collect()
    }

    /// Remove and shut down a platform by its ID.
    pub async fn unregister(&mut self, platform_id: &str) -> Option<Box<dyn MessagingPlatform>> {
        if let Some(platform) = self.platforms.remove(platform_id) {
            let _ = platform.shutdown().await;
            Some(platform)
        } else {
            None
        }
    }

    /// Create a merged event stream from all registered platforms.
    /// Uses futures_util SelectAll to combine streams from all platforms into one.
    pub async fn merged_event_stream(&self) -> Result<EventStream> {
        let mut select_all = SelectAll::<
            std::pin::Pin<Box<dyn futures_util::Stream<Item = IncomingMessage> + Send>>,
        >::new();
        for platform in self.platforms.values() {
            let stream = platform.event_stream().await?;
            select_all.push(stream);
        }
        Ok(Box::pin(select_all))
    }

    /// Gracefully shut down all platforms with a 10-second timeout.
    pub async fn shutdown(&self) {
        let futures: Vec<_> = self.platforms.values().map(|p| p.shutdown()).collect();

        let _ = tokio::time::timeout(
            Duration::from_secs(10),
            futures_util::future::join_all(futures),
        )
        .await;
    }

    /// Aggregate health from all registered platforms.
    /// Calls connection_state() on each platform through the MessagingPlatform trait.
    pub fn health(&self) -> Vec<PlatformHealth> {
        self.platforms
            .iter()
            .map(|(id, p)| PlatformHealth {
                platform_id: id.clone(),
                state: p.connection_state(),
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use gorp_core::MessageContent;
    use tokio_stream::StreamExt;

    struct MockPlatform {
        id: &'static str,
    }

    #[async_trait]
    impl MessagingPlatform for MockPlatform {
        async fn event_stream(&self) -> Result<EventStream> {
            Ok(Box::pin(tokio_stream::empty()))
        }

        async fn send(&self, _channel_id: &str, _content: MessageContent) -> Result<()> {
            Ok(())
        }

        fn bot_user_id(&self) -> &str {
            "bot"
        }

        fn platform_id(&self) -> &'static str {
            self.id
        }
    }

    struct MockPlatformWithState {
        id: &'static str,
        state: PlatformConnectionState,
    }

    #[async_trait]
    impl MessagingPlatform for MockPlatformWithState {
        async fn event_stream(&self) -> Result<EventStream> {
            Ok(Box::pin(tokio_stream::empty()))
        }

        async fn send(&self, _channel_id: &str, _content: MessageContent) -> Result<()> {
            Ok(())
        }

        fn bot_user_id(&self) -> &str {
            "bot"
        }

        fn platform_id(&self) -> &'static str {
            self.id
        }

        fn connection_state(&self) -> PlatformConnectionState {
            self.state.clone()
        }
    }

    #[test]
    fn test_empty_registry() {
        let registry = PlatformRegistry::new();
        assert!(registry.is_empty());
        assert_eq!(registry.len(), 0);
        assert!(registry.platform_ids().is_empty());
        assert!(registry.health().is_empty());
    }

    #[test]
    fn test_register_and_get_platform() {
        let mut registry = PlatformRegistry::new();
        registry.register(Box::new(MockPlatform { id: "matrix" }));

        assert!(!registry.is_empty());
        assert_eq!(registry.len(), 1);

        let platform = registry.get("matrix");
        assert!(platform.is_some());
        assert_eq!(platform.unwrap().platform_id(), "matrix");
    }

    #[test]
    fn test_get_nonexistent_platform() {
        let registry = PlatformRegistry::new();
        assert!(registry.get("nonexistent").is_none());
    }

    #[test]
    fn test_register_multiple_platforms() {
        let mut registry = PlatformRegistry::new();
        registry.register(Box::new(MockPlatform { id: "matrix" }));
        registry.register(Box::new(MockPlatform { id: "slack" }));
        registry.register(Box::new(MockPlatform { id: "telegram" }));

        assert_eq!(registry.len(), 3);

        assert!(registry.get("matrix").is_some());
        assert!(registry.get("slack").is_some());
        assert!(registry.get("telegram").is_some());
    }

    #[test]
    fn test_platform_ids() {
        let mut registry = PlatformRegistry::new();
        registry.register(Box::new(MockPlatform { id: "matrix" }));
        registry.register(Box::new(MockPlatform { id: "slack" }));

        let mut ids = registry.platform_ids();
        ids.sort();
        assert_eq!(ids, vec!["matrix".to_string(), "slack".to_string()]);
    }

    #[test]
    fn test_register_overwrites_duplicate_id() {
        let mut registry = PlatformRegistry::new();
        registry.register(Box::new(MockPlatform { id: "matrix" }));
        registry.register(Box::new(MockPlatform { id: "matrix" }));

        assert_eq!(registry.len(), 1);
    }

    #[test]
    fn test_health_reports_all_platforms() {
        let mut registry = PlatformRegistry::new();
        registry.register(Box::new(MockPlatform { id: "matrix" }));
        registry.register(Box::new(MockPlatform { id: "slack" }));

        let health = registry.health();
        assert_eq!(health.len(), 2);

        let mut platform_ids: Vec<_> = health.iter().map(|h| h.platform_id.clone()).collect();
        platform_ids.sort();
        assert_eq!(
            platform_ids,
            vec!["matrix".to_string(), "slack".to_string()]
        );

        // All platforms default to Connected state
        for h in &health {
            assert!(matches!(h.state, PlatformConnectionState::Connected));
        }
    }

    #[tokio::test]
    async fn test_merged_event_stream_empty_registry() {
        let registry = PlatformRegistry::new();
        let stream = registry.merged_event_stream().await;
        assert!(stream.is_ok());

        let mut stream = stream.unwrap();
        // Empty registry produces a stream that immediately ends
        let next = StreamExt::next(&mut stream).await;
        assert!(next.is_none());
    }

    #[tokio::test]
    async fn test_merged_event_stream_with_platforms() {
        let mut registry = PlatformRegistry::new();
        registry.register(Box::new(MockPlatform { id: "matrix" }));
        registry.register(Box::new(MockPlatform { id: "slack" }));

        let stream = registry.merged_event_stream().await;
        assert!(stream.is_ok());

        // Both MockPlatforms return empty streams, so merged stream ends immediately
        let mut stream = stream.unwrap();
        let next = StreamExt::next(&mut stream).await;
        assert!(next.is_none());
    }

    #[tokio::test]
    async fn test_shutdown_completes() {
        let mut registry = PlatformRegistry::new();
        registry.register(Box::new(MockPlatform { id: "matrix" }));
        registry.register(Box::new(MockPlatform { id: "slack" }));

        // shutdown should complete without panicking
        registry.shutdown().await;
    }

    #[test]
    fn test_platform_health_debug() {
        let health = PlatformHealth {
            platform_id: "matrix".to_string(),
            state: PlatformConnectionState::Connected,
        };
        let debug = format!("{:?}", health);
        assert!(debug.contains("matrix"));
        assert!(debug.contains("Connected"));
    }

    #[test]
    fn test_platform_health_clone() {
        let health = PlatformHealth {
            platform_id: "matrix".to_string(),
            state: PlatformConnectionState::Connected,
        };
        let cloned = health.clone();
        assert_eq!(cloned.platform_id, "matrix");
        assert!(matches!(cloned.state, PlatformConnectionState::Connected));
    }

    #[test]
    fn test_connection_state_default_returns_connected() {
        let platform = MockPlatform { id: "test" };
        assert!(matches!(
            platform.connection_state(),
            PlatformConnectionState::Connected
        ));
    }

    #[test]
    fn test_health_returns_actual_platform_state() {
        let mut registry = PlatformRegistry::new();
        registry.register(Box::new(MockPlatformWithState {
            id: "disconnected_one",
            state: PlatformConnectionState::Disconnected {
                reason: "timeout".to_string(),
            },
        }));
        registry.register(Box::new(MockPlatformWithState {
            id: "connected_one",
            state: PlatformConnectionState::Connected,
        }));

        let health = registry.health();
        assert_eq!(health.len(), 2);

        let disconnected = health.iter().find(|h| h.platform_id == "disconnected_one").unwrap();
        assert!(matches!(
            disconnected.state,
            PlatformConnectionState::Disconnected { .. }
        ));

        let connected = health.iter().find(|h| h.platform_id == "connected_one").unwrap();
        assert!(matches!(connected.state, PlatformConnectionState::Connected));
    }

    #[tokio::test]
    async fn test_unregister_removes_platform() {
        let mut registry = PlatformRegistry::new();
        registry.register(Box::new(MockPlatform { id: "matrix" }));
        registry.register(Box::new(MockPlatform { id: "slack" }));
        assert_eq!(registry.len(), 2);

        let removed = registry.unregister("matrix").await;
        assert!(removed.is_some());
        assert_eq!(removed.unwrap().platform_id(), "matrix");
        assert_eq!(registry.len(), 1);
        assert!(registry.get("matrix").is_none());
        assert!(registry.get("slack").is_some());
    }

    #[tokio::test]
    async fn test_unregister_nonexistent_returns_none() {
        let mut registry = PlatformRegistry::new();
        let removed = registry.unregister("nonexistent").await;
        assert!(removed.is_none());
    }
}
