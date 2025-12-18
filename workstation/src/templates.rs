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
