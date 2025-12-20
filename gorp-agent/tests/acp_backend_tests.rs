#[cfg(feature = "acp")]
mod acp_tests {
    use gorp_agent::backends::acp::{AcpBackend, AcpConfig};
    use serde_json::json;
    use std::path::PathBuf;

    #[test]
    fn test_acp_config_deserializes() {
        let json = json!({
            "binary": "codex-acp",
            "timeout_secs": 300,
            "working_dir": "/tmp"
        });
        let config: AcpConfig = serde_json::from_value(json).unwrap();
        assert_eq!(config.binary, "codex-acp");
        assert_eq!(config.timeout_secs, 300);
        assert_eq!(config.working_dir, PathBuf::from("/tmp"));
    }

    #[test]
    fn test_acp_config_with_defaults() {
        let json = json!({
            "binary": "claude-code-acp",
            "working_dir": "/workspace"
        });
        let config: AcpConfig = serde_json::from_value(json).unwrap();
        assert_eq!(config.binary, "claude-code-acp");
        assert_eq!(config.working_dir, PathBuf::from("/workspace"));
        // Default timeout should be set
        assert!(config.timeout_secs > 0);
    }

    #[tokio::test]
    async fn test_acp_backend_name() {
        let config = AcpConfig {
            binary: "codex-acp".to_string(),
            timeout_secs: 300,
            working_dir: PathBuf::from("/tmp"),
        };

        let backend = AcpBackend::new(config).unwrap();
        let handle = backend.into_handle();
        assert_eq!(handle.name(), "acp");
    }
}
