// ABOUTME: Matrix OIDC authentication for workstation webapp.
// ABOUTME: Handles login flow, token exchange, and session management.

use anyhow::{Context, Result};
use axum::{
    extract::{Query, State},
    response::Redirect,
};
use oauth2::{
    AuthorizationCode, CsrfToken, PkceCodeChallenge, PkceCodeVerifier, Scope, TokenResponse,
};
use serde::Deserialize;
use tower_sessions::Session;

use crate::AppState;

const OIDC_STATE_KEY: &str = "oidc_state";
const PKCE_VERIFIER_KEY: &str = "pkce_verifier";
pub const USER_KEY: &str = "matrix_user";

pub async fn login(session: Session, State(state): State<AppState>) -> Redirect {
    let (pkce_challenge, pkce_verifier) = PkceCodeChallenge::new_random_sha256();

    let (auth_url, csrf_token) = state
        .oidc
        .client
        .authorize_url(CsrfToken::new_random)
        .add_scope(Scope::new("openid".to_string()))
        .add_scope(Scope::new("email".to_string()))
        .set_pkce_challenge(pkce_challenge)
        .url();

    session
        .insert(OIDC_STATE_KEY, csrf_token.secret().clone())
        .await
        .ok();
    session
        .insert(PKCE_VERIFIER_KEY, pkce_verifier.secret().clone())
        .await
        .ok();

    Redirect::to(auth_url.as_str())
}

#[derive(Deserialize)]
pub struct CallbackParams {
    pub code: String,
    pub state: String,
}

pub async fn callback(
    session: Session,
    State(state): State<AppState>,
    Query(params): Query<CallbackParams>,
) -> Redirect {
    // Validate CSRF state
    let stored_state: Option<String> = session.get(OIDC_STATE_KEY).await.ok().flatten();

    if stored_state.as_deref() != Some(&params.state) {
        tracing::warn!("CSRF state mismatch");
        return Redirect::to("/?error=state_mismatch");
    }

    // Retrieve PKCE verifier
    let stored_verifier: Option<String> = session.get(PKCE_VERIFIER_KEY).await.ok().flatten();
    let stored_verifier = match stored_verifier {
        Some(v) => v,
        None => {
            tracing::error!("Missing PKCE verifier in session");
            return Redirect::to("/?error=missing_verifier");
        }
    };

    // Exchange authorization code for tokens
    match exchange_code_for_user(&state, &params.code, &stored_verifier).await {
        Ok(matrix_id) => {
            // Store Matrix ID in session
            if let Err(e) = session.insert(USER_KEY, matrix_id.clone()).await {
                tracing::error!(error = ?e, "Failed to store user in session");
                return Redirect::to("/?error=session_error");
            }

            // Clean up session keys
            session.remove::<String>(OIDC_STATE_KEY).await.ok();
            session.remove::<String>(PKCE_VERIFIER_KEY).await.ok();

            tracing::info!(matrix_id = %matrix_id, "User authenticated successfully");
            Redirect::to("/")
        }
        Err(e) => {
            tracing::error!(error = ?e, "OIDC callback failed");
            Redirect::to("/?error=auth_failed")
        }
    }
}

async fn exchange_code_for_user(
    state: &AppState,
    code: &str,
    verifier: &str,
) -> Result<String> {
    // Exchange code for tokens
    let token_response = state
        .oidc
        .client
        .exchange_code(AuthorizationCode::new(code.to_string()))
        .set_pkce_verifier(PkceCodeVerifier::new(verifier.to_string()))
        .request_async(oauth2::reqwest::async_http_client)
        .await
        .context("Failed to exchange authorization code for tokens")?;

    let access_token = token_response.access_token().secret();

    // Fetch userinfo from OIDC provider
    let http_client = reqwest::Client::new();
    let userinfo: serde_json::Value = http_client
        .get(&state.oidc.userinfo_endpoint)
        .bearer_auth(access_token)
        .send()
        .await
        .context("Failed to fetch userinfo")?
        .json()
        .await
        .context("Failed to parse userinfo response")?;

    // Extract Matrix ID from sub claim
    let matrix_id = userinfo
        .get("sub")
        .and_then(|v| v.as_str())
        .context("Missing sub claim in userinfo")?
        .to_string();

    tracing::debug!(matrix_id = %matrix_id, "Extracted Matrix ID from userinfo");

    Ok(matrix_id)
}

pub async fn logout(session: Session) -> Redirect {
    session.flush().await.ok();
    Redirect::to("/")
}

pub async fn get_current_user(session: &Session) -> Option<String> {
    session.get::<String>(USER_KEY).await.ok().flatten()
}
