// ABOUTME: Axum router setup for workstation webapp.
// ABOUTME: Defines all HTTP routes and middleware.

use askama::Template;
use axum::{
    extract::State,
    response::{Html, IntoResponse},
    routing::get,
    Router,
};
use tower_sessions::{MemoryStore, Session, SessionManagerLayer};

use crate::{
    auth::{self, get_current_user},
    files,
    templates::{IndexTemplate, TerminalTemplate},
    AppState,
};

pub fn create_router(state: AppState) -> Router {
    let session_store = MemoryStore::default();
    let session_layer = SessionManagerLayer::new(session_store)
        .with_secure(false)
        .with_same_site(tower_sessions::cookie::SameSite::Lax);

    Router::new()
        .route("/", get(index))
        .route("/health", get(health))
        .route("/terminal", get(terminal))
        .route("/auth/login", get(auth::login))
        .route("/auth/callback", get(auth::callback))
        .route("/auth/logout", get(auth::logout))
        .route("/files/{channel}/{*path}", get(files::list_files))
        .route(
            "/edit/{channel}/{*path}",
            get(files::read_file).post(files::save_file),
        )
        .layer(session_layer)
        .with_state(state)
}

async fn index(State(state): State<AppState>, session: Session) -> impl IntoResponse {
    let user = get_current_user(&session).await;

    let (channels, error) = if user.is_some() {
        match state.gorp.list_channels().await {
            Ok(c) => (c, None),
            Err(e) => (vec![], Some(e.to_string())),
        }
    } else {
        (vec![], None)
    };

    let template = IndexTemplate {
        user,
        channels,
        error,
    };
    Html(template.render().unwrap())
}

async fn health() -> &'static str {
    "ok"
}

async fn terminal(State(state): State<AppState>, session: Session) -> impl IntoResponse {
    let user = get_current_user(&session).await;

    // Convert http URL to ws URL
    let gorp_ws_url = state
        .config
        .gorp_api_url
        .replace("http://", "ws://")
        .replace("https://", "wss://");

    let template = TerminalTemplate {
        user,
        gorp_api_url: state.config.gorp_api_url.clone(),
        gorp_ws_url,
        workspace_path: state.config.workspace_path.clone(),
    };
    Html(template.render().unwrap())
}
