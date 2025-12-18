// ABOUTME: Admin panel module for web-based configuration management
// ABOUTME: Provides routes at /admin/* for config viewing and editing

pub mod auth;
pub mod routes;
pub mod templates;

pub use auth::auth_middleware;
pub use routes::{admin_router, AdminState};

// Re-export browser types for convenience
pub use crate::browser::BrowserManager;
