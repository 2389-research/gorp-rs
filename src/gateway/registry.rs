// ABOUTME: Registry that manages gateway adapter lifecycle.
// ABOUTME: Handles registration, lookup, and coordinated shutdown of all adapters.

use std::collections::HashMap;

use super::GatewayAdapter;

/// Central registry for all active gateway adapters.
///
/// Stores adapters keyed by platform_id and provides lookup, enumeration,
/// and coordinated shutdown across all registered adapters.
pub struct GatewayRegistry {
    adapters: HashMap<String, Box<dyn GatewayAdapter>>,
}

impl GatewayRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self {
            adapters: HashMap::new(),
        }
    }

    /// Register an adapter. If an adapter with the same platform_id already
    /// exists, it is replaced.
    pub fn register(&mut self, adapter: Box<dyn GatewayAdapter>) {
        let id = adapter.platform_id().to_string();
        self.adapters.insert(id, adapter);
    }

    /// Look up an adapter by platform_id.
    pub fn get(&self, platform_id: &str) -> Option<&dyn GatewayAdapter> {
        self.adapters.get(platform_id).map(|a| a.as_ref())
    }

    /// List all registered platform IDs.
    pub fn platform_ids(&self) -> Vec<String> {
        self.adapters.keys().cloned().collect()
    }

    /// Remove and return an adapter by platform_id.
    pub fn unregister(&mut self, platform_id: &str) -> Option<Box<dyn GatewayAdapter>> {
        self.adapters.remove(platform_id)
    }

    /// Stop all registered adapters and clear the registry.
    pub async fn shutdown_all(&mut self) {
        for (id, adapter) in self.adapters.drain() {
            if let Err(e) = adapter.stop().await {
                tracing::error!(platform_id = %id, error = %e, "gateway adapter shutdown failed");
            }
        }
    }
}

impl Default for GatewayRegistry {
    fn default() -> Self {
        Self::new()
    }
}
