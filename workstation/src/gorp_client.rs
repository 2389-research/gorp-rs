// ABOUTME: HTTP client for communicating with gorp API.
// ABOUTME: Fetches channel data and proxies requests.

use anyhow::Result;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Channel {
    pub channel_name: String,
    pub room_id: String,
    pub session_id: String,
    pub directory: String,
    pub started: bool,
    pub created_at: String,
}

#[derive(Clone)]
pub struct GorpClient {
    base_url: String,
    client: reqwest::Client,
}

impl GorpClient {
    pub fn new(base_url: &str) -> Self {
        Self {
            base_url: base_url.to_string(),
            client: reqwest::Client::new(),
        }
    }

    pub async fn list_channels(&self) -> Result<Vec<Channel>> {
        let url = format!("{}/api/channels", self.base_url);
        let response = self.client.get(&url).send().await?;

        if !response.status().is_success() {
            anyhow::bail!("Failed to fetch channels: {}", response.status());
        }

        let channels: Vec<Channel> = response.json().await?;
        Ok(channels)
    }
}
