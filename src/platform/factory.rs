// ABOUTME: Platform factory for hot-connecting gateways at runtime
// ABOUTME: Creates platform instances from config for Telegram and Slack

use anyhow::Result;
use gorp_core::MessagingPlatform;

use crate::config::Config;

/// Create a platform instance from the current config.
/// Supports hot-connect for Telegram and Slack.
/// Matrix requires complex setup (encryption, device verification) and is not supported.
/// WhatsApp uses a sidecar process and is not supported.
pub async fn create_platform(
    #[allow(unused_variables)] config: &Config,
    platform_id: &str,
) -> Result<Box<dyn MessagingPlatform>> {
    match platform_id {
        #[cfg(feature = "telegram")]
        "telegram" => {
            let tg_config = config
                .telegram
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("Telegram not configured. Save config first."))?;
            let platform = super::TelegramPlatform::new(tg_config.clone()).await?;
            Ok(Box::new(platform))
        }
        #[cfg(not(feature = "telegram"))]
        "telegram" => {
            anyhow::bail!("Telegram support not compiled. Build with --features telegram")
        }
        #[cfg(feature = "slack")]
        "slack" => {
            let slack_config = config
                .slack
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("Slack not configured. Save config first."))?;
            let platform = super::SlackPlatform::new(slack_config.clone()).await?;
            Ok(Box::new(platform))
        }
        #[cfg(not(feature = "slack"))]
        "slack" => {
            anyhow::bail!("Slack support not compiled. Build with --features slack")
        }
        "matrix" => {
            anyhow::bail!(
                "Matrix requires complex setup (encryption, device verification). \
                 Please restart gorp to connect Matrix."
            )
        }
        "whatsapp" => {
            anyhow::bail!(
                "WhatsApp uses a sidecar process and cannot be hot-connected. \
                 Please restart gorp to connect WhatsApp."
            )
        }
        _ => anyhow::bail!("Unknown platform: {}", platform_id),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> Config {
        // Minimal Config with no platforms configured
        toml::from_str(
            r#"
            [webhook]
            port = 9999
            host = "localhost"
            [workspace]
            path = "/tmp/gorp-test"
            "#,
        )
        .unwrap()
    }

    #[tokio::test]
    async fn test_factory_rejects_matrix() {
        let config = test_config();
        let result = create_platform(&config, "matrix").await;
        let err = result.err().expect("should error for matrix");
        assert!(err.to_string().contains("restart gorp"));
    }

    #[tokio::test]
    async fn test_factory_rejects_whatsapp() {
        let config = test_config();
        let result = create_platform(&config, "whatsapp").await;
        let err = result.err().expect("should error for whatsapp");
        assert!(err.to_string().contains("sidecar"));
    }

    #[tokio::test]
    async fn test_factory_rejects_unknown() {
        let config = test_config();
        let result = create_platform(&config, "discord").await;
        let err = result.err().expect("should error for unknown");
        assert!(err.to_string().contains("Unknown platform"));
    }
}
