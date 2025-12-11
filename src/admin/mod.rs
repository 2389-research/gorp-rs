// ABOUTME: Admin panel module for web-based configuration management
// ABOUTME: Provides routes at /admin/* for config viewing and editing

pub mod routes;
pub mod templates;

pub use routes::{admin_router, AdminState};
