// ABOUTME: Axum router setup for workstation webapp.
// ABOUTME: Defines all HTTP routes and middleware.

use askama::Template;
use axum::{
    response::{Html, IntoResponse},
    routing::get,
    Router,
};
use tower_sessions::{MemoryStore, SessionManagerLayer};

use crate::templates::IndexTemplate;

pub fn create_router() -> Router {
    let session_store = MemoryStore::default();
    let session_layer = SessionManagerLayer::new(session_store)
        .with_secure(false)
        .with_same_site(tower_sessions::cookie::SameSite::Lax);

    Router::new()
        .route("/", get(index))
        .route("/health", get(health))
        .layer(session_layer)
}

async fn index() -> impl IntoResponse {
    let template = IndexTemplate;
    Html(template.render().unwrap())
}

async fn health() -> &'static str {
    "ok"
}
