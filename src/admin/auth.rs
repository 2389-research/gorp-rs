// ABOUTME: Simple authentication for admin panel
// ABOUTME: Uses API key or allows localhost access if no key configured

use axum::{
    extract::{ConnectInfo, Request, State},
    http::StatusCode,
    middleware::Next,
    response::Response,
};
use std::net::SocketAddr;

use super::AdminState;

pub async fn auth_middleware(
    State(state): State<AdminState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let api_key = &state.config.webhook.api_key;

    // If no API key configured, only allow localhost
    if api_key.is_none() {
        let is_localhost = addr.ip().is_loopback();
        if !is_localhost {
            tracing::warn!(remote_addr = %addr, "Admin access denied: no API key and not localhost");
            return Err(StatusCode::FORBIDDEN);
        }
        return Ok(next.run(request).await);
    }

    // Check for API key in query params or header
    let uri = request.uri();
    let query = uri.query().unwrap_or("");
    let has_valid_key = query.contains(&format!("key={}", api_key.as_ref().unwrap()))
        || request
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
