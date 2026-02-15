// ABOUTME: Admin panel module for web-based configuration management
// ABOUTME: Provides routes at /admin/* for config viewing and editing

pub mod auth;
pub mod routes;
pub mod setup;
pub mod templates;
pub mod websocket;

pub use auth::{auth_middleware, setup_guard_middleware, AuthConfig};
pub use routes::{admin_router, AdminState};
pub use setup::{login_router, setup_router};
pub use websocket::{ws_handler, WsHub};
