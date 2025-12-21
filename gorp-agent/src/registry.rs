// ABOUTME: Registry pattern for runtime backend selection.
// ABOUTME: Backends register factories, gorp creates by name from config.

use crate::handle::AgentHandle;
use anyhow::{anyhow, Result};
use serde_json::Value;
use std::collections::HashMap;

/// Factory function that creates an AgentHandle from config
pub type BackendFactory = Box<dyn Fn(&Value) -> Result<AgentHandle> + Send + Sync>;

/// Registry for runtime backend selection
pub struct AgentRegistry {
    factories: HashMap<String, BackendFactory>,
}

impl AgentRegistry {
    /// Create a new empty registry
    pub fn new() -> Self {
        Self {
            factories: HashMap::new(),
        }
    }

    /// Register a backend factory by name
    pub fn register<F>(mut self, name: &str, factory: F) -> Self
    where
        F: Fn(&Value) -> Result<AgentHandle> + Send + Sync + 'static,
    {
        self.factories.insert(name.to_string(), Box::new(factory));
        self
    }

    /// Create a backend by name with the given config
    pub fn create(&self, name: &str, config: &Value) -> Result<AgentHandle> {
        let factory = self
            .factories
            .get(name)
            .ok_or_else(|| anyhow!("Unknown backend: {}", name))?;
        factory(config)
    }

    /// List available backend names
    pub fn available(&self) -> Vec<&str> {
        self.factories.keys().map(|s| s.as_str()).collect()
    }

    /// Create a backend from a BackendConfig
    pub fn create_from_config(&self, config: &crate::config::BackendConfig) -> Result<AgentHandle> {
        let json_config = config.to_json_value();
        self.create(config.backend_type(), &json_config)
    }
}

impl Default for AgentRegistry {
    fn default() -> Self {
        use crate::backends::direct_cli::DirectCliBackend;
        use crate::backends::mock::MockBackend;

        let registry = Self::new()
            .register("mock", MockBackend::factory())
            .register("direct", DirectCliBackend::factory());

        #[cfg(feature = "acp")]
        let registry = {
            use crate::backends::acp::AcpBackend;
            registry.register("acp", AcpBackend::factory())
        };

        registry
    }
}
