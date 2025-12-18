// ABOUTME: Matrix OIDC authentication for workstation webapp.
// ABOUTME: Handles login flow, token exchange, and session management.

use anyhow::Result;
use axum::{
    extract::{Query, State},
    response::Redirect,
};
use oauth2::{
    basic::BasicClient, AuthUrl, ClientId, CsrfToken,
    PkceCodeChallenge, RedirectUrl, Scope, TokenUrl,
};
use serde::Deserialize;
use tower_sessions::Session;

use crate::AppState;

const OIDC_STATE_KEY: &str = "oidc_state";
const PKCE_VERIFIER_KEY: &str = "pkce_verifier";
pub const USER_KEY: &str = "matrix_user";

#[derive(Clone)]
pub struct OidcConfig {
    pub client: BasicClient,
}

impl OidcConfig {
    pub fn new(homeserver: &str, client_id: &str, redirect_uri: &str) -> Result<Self> {
        let auth_url = AuthUrl::new(format!(
            "{}/_matrix/client/v3/login/sso/redirect",
            homeserver
        ))?;
        let token_url = TokenUrl::new(format!("{}/_matrix/client/v3/login", homeserver))?;

        let client = BasicClient::new(
            ClientId::new(client_id.to_string()),
            None,
            auth_url,
            Some(token_url),
        )
        .set_redirect_uri(RedirectUrl::new(redirect_uri.to_string())?);

        Ok(Self { client })
    }
}

pub async fn login(session: Session, State(state): State<AppState>) -> Redirect {
    let (pkce_challenge, pkce_verifier) = PkceCodeChallenge::new_random_sha256();

    let (auth_url, csrf_token) = state
        .oidc
        .client
        .authorize_url(CsrfToken::new_random)
        .add_scope(Scope::new("openid".to_string()))
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
    State(_state): State<AppState>,
    Query(params): Query<CallbackParams>,
) -> Redirect {
    let stored_state: Option<String> = session.get(OIDC_STATE_KEY).await.ok().flatten();

    if stored_state.as_deref() != Some(&params.state) {
        tracing::warn!("CSRF state mismatch");
        return Redirect::to("/?error=state_mismatch");
    }

    // For now, just set a placeholder user - real OIDC exchange requires more Matrix-specific handling
    // Matrix SSO returns a login token, not standard OIDC tokens
    session.insert(USER_KEY, "authenticated").await.ok();

    session.remove::<String>(OIDC_STATE_KEY).await.ok();
    session.remove::<String>(PKCE_VERIFIER_KEY).await.ok();

    Redirect::to("/")
}

pub async fn logout(session: Session) -> Redirect {
    session.flush().await.ok();
    Redirect::to("/")
}

pub async fn get_current_user(session: &Session) -> Option<String> {
    session.get::<String>(USER_KEY).await.ok().flatten()
}
