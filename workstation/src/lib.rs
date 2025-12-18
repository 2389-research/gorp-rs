// ABOUTME: Workstation webapp library - user-facing config UI for gorp workspaces.
// ABOUTME: Provides file management, terminal access, and browser viewer.

pub mod auth;
pub mod config;
pub mod files;
pub mod gorp_client;
pub mod oidc;
pub mod routes;
pub mod templates;

use gorp_client::GorpClient;
use oidc::OidcConfig;

#[derive(Clone)]
pub struct AppState {
    pub config: config::Config,
    pub oidc: OidcConfig,
    pub gorp: GorpClient,
}
