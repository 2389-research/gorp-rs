// ABOUTME: Axum router setup for workstation webapp.
// ABOUTME: Defines all HTTP routes and middleware.

use askama::Template;
use axum::{
    response::{Html, IntoResponse},
    routing::get,
    Router,
};

use crate::templates::IndexTemplate;

pub fn create_router() -> Router {
    Router::new()
        .route("/", get(index))
        .route("/health", get(health))
}

async fn index() -> impl IntoResponse {
    let template = IndexTemplate;
    Html(template.render().unwrap())
}

async fn health() -> &'static str {
    "ok"
}
