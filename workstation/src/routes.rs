// ABOUTME: Axum router setup for workstation webapp.
// ABOUTME: Defines all HTTP routes and middleware.

use askama::Template;
use axum::{
    response::{Html, IntoResponse},
    routing::get,
    Router,
};
use tower_sessions::{MemoryStore, Session, SessionManagerLayer};

use crate::{
    auth::{self, get_current_user},
    files,
    templates::IndexTemplate,
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

#[axum::debug_handler]
async fn index(session: Session) -> impl IntoResponse {
    let user = get_current_user(&session).await;
    let template = IndexTemplate { user };
    Html(template.render().unwrap())
}

async fn health() -> &'static str {
    "ok"
}
