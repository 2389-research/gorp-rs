// ABOUTME: Askama template definitions for workstation webapp.
// ABOUTME: Defines structs that map to HTML templates.

use askama::Template;

use crate::gorp_client::Channel;

#[derive(Template)]
#[template(path = "index.html")]
pub struct IndexTemplate {
    pub user: Option<String>,
    pub channels: Vec<Channel>,
    pub error: Option<String>,
}

#[derive(Template)]
#[template(path = "terminal.html")]
pub struct TerminalTemplate {
    pub user: Option<String>,
    pub gorp_api_url: String,
    pub gorp_ws_url: String,
    pub workspace_path: String,
}
