// ABOUTME: Authentication system for the admin panel with setup wizard support
// ABOUTME: Supports username/password login, API token auth, and first-run setup flow

use anyhow::{Context, Result};
use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use axum::{
    extract::{ConnectInfo, Request, State},
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Redirect, Response},
};
use rand::Rng;
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use super::AdminState;

/// API token prefix for easy identification in logs and configs
const TOKEN_PREFIX: &str = "gorp_tk_";

/// Length of the random hex portion of the API token
const TOKEN_HEX_LEN: usize = 32;

// =============================================================================
// AuthConfig — persisted to data/auth.toml
// =============================================================================

/// Authentication configuration stored separately from the main config
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthConfig {
    /// Admin username
    pub username: String,
    /// Argon2-hashed password
    pub password_hash: String,
    /// API token with gorp_tk_ prefix for programmatic access
    pub api_token: String,
    /// Whether initial setup has been completed
    pub setup_complete: bool,
}

impl AuthConfig {
    /// Create a new AuthConfig by hashing the provided password
    pub fn create(username: &str, password: &str) -> Result<Self> {
        let password_hash = hash_password(password)?;
        let api_token = generate_api_token();

        Ok(Self {
            username: username.to_string(),
            password_hash,
            api_token,
            setup_complete: true,
        })
    }

    /// Verify a password against the stored hash
    pub fn verify_password(&self, password: &str) -> bool {
        verify_password(password, &self.password_hash)
    }

    /// Check if an API token matches
    pub fn verify_token(&self, token: &str) -> bool {
        // Constant-time comparison to prevent timing attacks
        constant_time_eq(token.as_bytes(), self.api_token.as_bytes())
    }

    /// Path to the auth config file
    pub fn config_path(data_dir: &str) -> PathBuf {
        Path::new(data_dir).join("auth.toml")
    }

    /// Load auth config from disk, returning None if it doesn't exist
    pub fn load(data_dir: &str) -> Result<Option<Self>> {
        let path = Self::config_path(data_dir);
        if !path.exists() {
            return Ok(None);
        }

        let content =
            std::fs::read_to_string(&path).context("Failed to read auth config file")?;

        let config: AuthConfig =
            toml::from_str(&content).context("Failed to parse auth config")?;

        Ok(Some(config))
    }

    /// Save auth config to disk
    pub fn save(&self, data_dir: &str) -> Result<()> {
        let path = Self::config_path(data_dir);

        // Ensure data directory exists
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).context("Failed to create data directory")?;
        }

        let content = toml::to_string_pretty(self).context("Failed to serialize auth config")?;
        std::fs::write(&path, content).context("Failed to write auth config")?;

        tracing::info!(path = %path.display(), "Auth config saved");
        Ok(())
    }
}

// =============================================================================
// Password hashing utilities
// =============================================================================

/// Hash a password using Argon2id
fn hash_password(password: &str) -> Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    let argon2 = Argon2::default();
    let hash = argon2
        .hash_password(password.as_bytes(), &salt)
        .map_err(|e| anyhow::anyhow!("Failed to hash password: {}", e))?;
    Ok(hash.to_string())
}

/// Verify a password against an Argon2 hash
fn verify_password(password: &str, hash: &str) -> bool {
    let parsed_hash = match PasswordHash::new(hash) {
        Ok(h) => h,
        Err(_) => return false,
    };
    Argon2::default()
        .verify_password(password.as_bytes(), &parsed_hash)
        .is_ok()
}

/// Generate a random API token with the gorp_tk_ prefix
fn generate_api_token() -> String {
    let mut rng = rand::thread_rng();
    let hex: String = (0..TOKEN_HEX_LEN)
        .map(|_| format!("{:x}", rng.gen::<u8>() & 0xf))
        .collect();
    format!("{}{}", TOKEN_PREFIX, hex)
}

/// Constant-time byte comparison to prevent timing attacks
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

// =============================================================================
// Middleware
// =============================================================================

/// Auth middleware that checks session cookies and API tokens.
///
/// Authentication flow:
/// 1. Check X-API-Key header → if valid token, proceed
/// 2. Check session cookie → if valid session, proceed
/// 3. If no auth config exists (pre-setup), allow localhost only
/// 4. Otherwise, reject with 401
pub async fn auth_middleware(
    State(state): State<AdminState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    // If auth config is loaded, check token or session
    if let Some(ref auth_config) = state.auth_config {
        // Check API token in X-API-Key header
        if let Some(token) = request
            .headers()
            .get("X-API-Key")
            .and_then(|v| v.to_str().ok())
        {
            if auth_config.verify_token(token) {
                return Ok(next.run(request).await);
            }
            tracing::warn!(remote_addr = %addr, "Admin access denied: invalid API token");
            return Err(StatusCode::UNAUTHORIZED);
        }

        // Check session cookie (via tower-sessions)
        // The session is validated by tower-sessions middleware before this runs.
        // We check for a "user" key in the session data.
        if let Some(session) = request.extensions().get::<tower_sessions::Session>() {
            if let Ok(Some(username)) = session.get::<String>("user").await {
                if username == auth_config.username {
                    return Ok(next.run(request).await);
                }
            }
        }

        // Fall back to legacy API key from webhook config
        if let Some(ref api_key) = state.config.webhook.api_key {
            if let Some(header_key) = request
                .headers()
                .get("X-API-Key")
                .and_then(|v| v.to_str().ok())
            {
                if header_key == api_key {
                    return Ok(next.run(request).await);
                }
            }
        }

        // No valid auth found
        tracing::warn!(remote_addr = %addr, "Admin access denied: no valid credentials");
        return Err(StatusCode::UNAUTHORIZED);
    }

    // No auth config exists — fall back to legacy behavior (localhost or webhook API key)
    let api_key = &state.config.webhook.api_key;

    if api_key.is_none() {
        let is_localhost = addr.ip().is_loopback();
        if !is_localhost {
            tracing::warn!(remote_addr = %addr, "Admin access denied: no API key and not localhost");
            return Err(StatusCode::FORBIDDEN);
        }
        return Ok(next.run(request).await);
    }

    // Check for API key in X-API-Key header
    let has_valid_key = request
        .headers()
        .get("X-API-Key")
        .and_then(|v| v.to_str().ok())
        == api_key.as_deref();

    if has_valid_key {
        Ok(next.run(request).await)
    } else {
        tracing::warn!(remote_addr = %addr, "Admin access denied: invalid API key");
        Err(StatusCode::UNAUTHORIZED)
    }
}

/// Setup wizard middleware that redirects ALL requests to /setup when
/// setup has not been completed yet. This runs before the auth middleware.
///
/// Paths that are always allowed (even pre-setup):
/// - /setup and /setup/* (the wizard itself)
/// - /health (monitoring)
/// - Static assets
pub async fn setup_guard_middleware(
    State(state): State<AdminState>,
    request: Request,
    next: Next,
) -> Response {
    // Check whether setup has been completed
    let setup_complete = state
        .auth_config
        .as_ref()
        .map_or(false, |c| c.setup_complete);

    if setup_complete {
        return next.run(request).await;
    }

    // Setup is not complete — allow setup routes and health checks through
    let path = request.uri().path();
    if path.starts_with("/setup")
        || path.starts_with("/admin/health")
        || path.starts_with("/static")
    {
        return next.run(request).await;
    }

    // Redirect everything else to setup
    Redirect::temporary("/setup").into_response()
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash_and_verify_password() {
        let password = "my-secure-password-123";
        let hash = hash_password(password).unwrap();

        assert!(verify_password(password, &hash));
        assert!(!verify_password("wrong-password", &hash));
    }

    #[test]
    fn test_hash_password_produces_unique_hashes() {
        let password = "same-password";
        let hash1 = hash_password(password).unwrap();
        let hash2 = hash_password(password).unwrap();

        // Different salts produce different hashes
        assert_ne!(hash1, hash2);

        // But both verify correctly
        assert!(verify_password(password, &hash1));
        assert!(verify_password(password, &hash2));
    }

    #[test]
    fn test_generate_api_token_format() {
        let token = generate_api_token();
        assert!(token.starts_with("gorp_tk_"));
        assert_eq!(token.len(), TOKEN_PREFIX.len() + TOKEN_HEX_LEN);
    }

    #[test]
    fn test_generate_api_token_uniqueness() {
        let token1 = generate_api_token();
        let token2 = generate_api_token();
        assert_ne!(token1, token2);
    }

    #[test]
    fn test_auth_config_create() {
        let config = AuthConfig::create("admin", "password123").unwrap();
        assert_eq!(config.username, "admin");
        assert!(config.setup_complete);
        assert!(config.api_token.starts_with("gorp_tk_"));
        assert!(config.verify_password("password123"));
        assert!(!config.verify_password("wrong"));
    }

    #[test]
    fn test_auth_config_verify_token() {
        let config = AuthConfig::create("admin", "pass").unwrap();
        let token = config.api_token.clone();
        assert!(config.verify_token(&token));
        assert!(!config.verify_token("gorp_tk_invalid"));
        assert!(!config.verify_token(""));
    }

    #[test]
    fn test_auth_config_serialize_roundtrip() {
        let config = AuthConfig::create("testuser", "testpass").unwrap();
        let serialized = toml::to_string_pretty(&config).unwrap();
        let deserialized: AuthConfig = toml::from_str(&serialized).unwrap();

        assert_eq!(deserialized.username, config.username);
        assert_eq!(deserialized.password_hash, config.password_hash);
        assert_eq!(deserialized.api_token, config.api_token);
        assert_eq!(deserialized.setup_complete, config.setup_complete);
    }

    #[test]
    fn test_auth_config_save_and_load() {
        let tmpdir = tempfile::tempdir().unwrap();
        let data_dir = tmpdir.path().to_str().unwrap();

        let config = AuthConfig::create("admin", "secret").unwrap();
        config.save(data_dir).unwrap();

        let loaded = AuthConfig::load(data_dir).unwrap().unwrap();
        assert_eq!(loaded.username, "admin");
        assert_eq!(loaded.api_token, config.api_token);
        assert!(loaded.verify_password("secret"));
    }

    #[test]
    fn test_auth_config_load_nonexistent() {
        let tmpdir = tempfile::tempdir().unwrap();
        let data_dir = tmpdir.path().to_str().unwrap();

        let loaded = AuthConfig::load(data_dir).unwrap();
        assert!(loaded.is_none());
    }

    #[test]
    fn test_auth_config_path() {
        let path = AuthConfig::config_path("/opt/gorp/data");
        assert_eq!(path, PathBuf::from("/opt/gorp/data/auth.toml"));
    }

    #[test]
    fn test_constant_time_eq() {
        assert!(constant_time_eq(b"hello", b"hello"));
        assert!(!constant_time_eq(b"hello", b"world"));
        assert!(!constant_time_eq(b"hello", b"hell"));
        assert!(!constant_time_eq(b"", b"x"));
        assert!(constant_time_eq(b"", b""));
    }

    #[test]
    fn test_password_hash_contains_argon2_marker() {
        let hash = hash_password("test").unwrap();
        assert!(hash.starts_with("$argon2"));
    }

    #[test]
    fn test_verify_password_rejects_invalid_hash() {
        assert!(!verify_password("test", "not-a-valid-hash"));
        assert!(!verify_password("test", ""));
    }
}
