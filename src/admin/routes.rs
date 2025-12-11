// ABOUTME: Admin panel route handlers
// ABOUTME: Handles config viewing, editing, and session management

use axum::{routing::get, Router};

use crate::admin::templates::DashboardTemplate;

/// Build the admin router mounted at /admin
pub fn admin_router() -> Router {
    Router::new()
        .route("/", get(dashboard))
}

async fn dashboard() -> DashboardTemplate {
    DashboardTemplate {
        title: "gorp Admin".to_string(),
    }
}
