// ABOUTME: Askama template definitions for workstation webapp.
// ABOUTME: Defines structs that map to HTML templates.

use askama::Template;

#[derive(Template)]
#[template(path = "index.html")]
pub struct IndexTemplate {
    pub user: Option<String>,
}
