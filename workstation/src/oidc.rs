// ABOUTME: OIDC discovery and dynamic client registration for Matrix auth.
// ABOUTME: Fetches endpoints from .well-known/openid-configuration and registers clients.

use anyhow::{Context, Result};
use oauth2::{
    basic::BasicClient, AuthUrl, ClientId, RedirectUrl, TokenUrl,
};
use serde::{Deserialize, Serialize};
use std::path::Path;
use tokio::fs;

/// OIDC discovery document from .well-known/openid-configuration
#[derive(Debug, Deserialize)]
pub struct OidcDiscovery {
    pub issuer: String,
    pub authorization_endpoint: String,
    pub token_endpoint: String,
    pub userinfo_endpoint: String,
    pub registration_endpoint: Option<String>,
    pub jwks_uri: String,
}

/// Response from dynamic client registration
#[derive(Debug, Deserialize, Serialize)]
pub struct ClientRegistration {
    pub client_id: String,
    pub client_secret: Option<String>,
    pub registration_access_token: Option<String>,
    pub registration_client_uri: Option<String>,
}

/// OIDC configuration with discovered endpoints and registered client
#[derive(Clone)]
pub struct OidcConfig {
    pub client: BasicClient,
    pub userinfo_endpoint: String,
    pub discovery: OidcDiscoveryData,
}

#[derive(Clone)]
pub struct OidcDiscoveryData {
    pub issuer: String,
    pub authorization_endpoint: String,
    pub token_endpoint: String,
    pub userinfo_endpoint: String,
}

impl OidcConfig {
    /// Initialize OIDC config by discovering endpoints and registering client
    pub async fn init(issuer: &str, redirect_uri: &str, cache_path: &str) -> Result<Self> {
        let http_client = reqwest::Client::new();

        // Step 1: Fetch discovery document
        let discovery_url = format!("{}/.well-known/openid-configuration", issuer.trim_end_matches('/'));
        let discovery: OidcDiscovery = http_client
            .get(&discovery_url)
            .send()
            .await
            .context("Failed to fetch OIDC discovery")?
            .json()
            .await
            .context("Failed to parse OIDC discovery")?;

        tracing::info!(issuer = %discovery.issuer, "OIDC discovery complete");

        // Step 2: Get or register client
        let registration = Self::get_or_register_client(
            &http_client,
            &discovery,
            redirect_uri,
            cache_path,
        ).await?;

        // Step 3: Build OAuth2 client
        let client = BasicClient::new(
            ClientId::new(registration.client_id),
            None,
            AuthUrl::new(discovery.authorization_endpoint.clone())?,
            Some(TokenUrl::new(discovery.token_endpoint.clone())?),
        )
        .set_redirect_uri(RedirectUrl::new(redirect_uri.to_string())?);

        Ok(Self {
            client,
            userinfo_endpoint: discovery.userinfo_endpoint.clone(),
            discovery: OidcDiscoveryData {
                issuer: discovery.issuer,
                authorization_endpoint: discovery.authorization_endpoint,
                token_endpoint: discovery.token_endpoint,
                userinfo_endpoint: discovery.userinfo_endpoint,
            },
        })
    }

    /// Get cached client registration or register new client
    async fn get_or_register_client(
        http_client: &reqwest::Client,
        discovery: &OidcDiscovery,
        redirect_uri: &str,
        cache_path: &str,
    ) -> Result<ClientRegistration> {
        let cache_file = Path::new(cache_path).join("oidc_client.json");

        // Try to load cached registration
        if cache_file.exists() {
            if let Ok(contents) = fs::read_to_string(&cache_file).await {
                if let Ok(registration) = serde_json::from_str::<ClientRegistration>(&contents) {
                    tracing::info!(client_id = %registration.client_id, "Using cached OIDC client");
                    return Ok(registration);
                }
            }
        }

        // Register new client
        let registration_endpoint = discovery.registration_endpoint.as_ref()
            .context("OIDC provider does not support dynamic client registration")?;

        let registration_request = serde_json::json!({
            "client_name": "Gorp Workstation",
            "redirect_uris": [redirect_uri],
            "token_endpoint_auth_method": "none",
            "grant_types": ["authorization_code", "refresh_token"],
            "response_types": ["code"]
        });

        let response = http_client
            .post(registration_endpoint)
            .json(&registration_request)
            .send()
            .await
            .context("Failed to register OIDC client")?;

        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_default();
            anyhow::bail!("Client registration failed: {}", error_text);
        }

        let registration: ClientRegistration = response
            .json()
            .await
            .context("Failed to parse client registration response")?;

        tracing::info!(client_id = %registration.client_id, "Registered new OIDC client");

        // Cache the registration
        if let Some(parent) = cache_file.parent() {
            fs::create_dir_all(parent).await.ok();
        }
        if let Ok(json) = serde_json::to_string_pretty(&registration) {
            fs::write(&cache_file, json).await.ok();
        }

        Ok(registration)
    }
}
