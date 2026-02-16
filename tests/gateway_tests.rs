// ABOUTME: Tests for the GatewayAdapter trait and GatewayRegistry.
// ABOUTME: Validates adapter registration, lookup, unregistration, and coordinated shutdown.

use async_trait::async_trait;
use gorp::bus::*;
use gorp::gateway::registry::GatewayRegistry;
use gorp::gateway::GatewayAdapter;
use std::sync::Arc;

struct MockAdapter {
    id: String,
}

#[async_trait]
impl GatewayAdapter for MockAdapter {
    fn platform_id(&self) -> &str {
        &self.id
    }

    async fn start(&self, _bus: Arc<MessageBus>) -> anyhow::Result<()> {
        Ok(())
    }

    async fn send(&self, _channel_id: &str, _content: ResponseContent) -> anyhow::Result<()> {
        Ok(())
    }

    async fn stop(&self) -> anyhow::Result<()> {
        Ok(())
    }
}

#[tokio::test]
async fn test_registry_register_and_get() {
    let mut registry = GatewayRegistry::new();
    let adapter = MockAdapter {
        id: "test".to_string(),
    };
    registry.register(Box::new(adapter));
    assert!(registry.get("test").is_some());
    assert!(registry.get("nonexistent").is_none());
}

#[tokio::test]
async fn test_registry_platform_ids() {
    let mut registry = GatewayRegistry::new();
    registry.register(Box::new(MockAdapter {
        id: "matrix".to_string(),
    }));
    registry.register(Box::new(MockAdapter {
        id: "slack".to_string(),
    }));
    let ids = registry.platform_ids();
    assert_eq!(ids.len(), 2);
    assert!(ids.contains(&"matrix".to_string()));
    assert!(ids.contains(&"slack".to_string()));
}

#[tokio::test]
async fn test_registry_unregister() {
    let mut registry = GatewayRegistry::new();
    registry.register(Box::new(MockAdapter {
        id: "matrix".to_string(),
    }));
    assert!(registry.get("matrix").is_some());
    let removed = registry.unregister("matrix");
    assert!(removed.is_some());
    assert!(registry.get("matrix").is_none());
}

#[tokio::test]
async fn test_registry_unregister_nonexistent() {
    let mut registry = GatewayRegistry::new();
    let removed = registry.unregister("nonexistent");
    assert!(removed.is_none());
}

#[tokio::test]
async fn test_registry_shutdown_all() {
    let mut registry = GatewayRegistry::new();
    registry.register(Box::new(MockAdapter {
        id: "a".to_string(),
    }));
    registry.register(Box::new(MockAdapter {
        id: "b".to_string(),
    }));
    registry.shutdown_all().await;
}

#[tokio::test]
async fn test_registry_register_overwrites_existing() {
    let mut registry = GatewayRegistry::new();
    registry.register(Box::new(MockAdapter {
        id: "matrix".to_string(),
    }));
    // Registering again with same platform_id overwrites
    registry.register(Box::new(MockAdapter {
        id: "matrix".to_string(),
    }));
    let ids = registry.platform_ids();
    assert_eq!(ids.len(), 1);
    assert!(ids.contains(&"matrix".to_string()));
}

#[tokio::test]
async fn test_registry_empty() {
    let registry = GatewayRegistry::new();
    assert!(registry.platform_ids().is_empty());
    assert!(registry.get("anything").is_none());
}

#[tokio::test]
async fn test_adapter_start_with_bus() {
    let adapter = MockAdapter {
        id: "test".to_string(),
    };
    let bus = Arc::new(MessageBus::new(64));
    let result = adapter.start(bus).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_adapter_send() {
    let adapter = MockAdapter {
        id: "test".to_string(),
    };
    let result = adapter
        .send("channel-1", ResponseContent::Complete("hello".to_string()))
        .await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_adapter_stop() {
    let adapter = MockAdapter {
        id: "test".to_string(),
    };
    let result = adapter.stop().await;
    assert!(result.is_ok());
}
