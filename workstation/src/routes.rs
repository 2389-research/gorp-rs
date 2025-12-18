// ABOUTME: Axum router setup for workstation webapp.
// ABOUTME: Defines all HTTP routes and middleware.

use axum::{routing::get, Router};

pub fn create_router() -> Router {
    Router::new()
        .route("/", get(index))
        .route("/health", get(health))
}

async fn index() -> &'static str {
    "Workstation"
}

async fn health() -> &'static str {
    "ok"
}
