// ABOUTME: File management for workspace directories.
// ABOUTME: Provides list, read, write, delete operations with path safety.

use anyhow::{bail, Result};
use axum::{
    extract::{Path, State},
    response::{Html, IntoResponse},
};
use askama::Template;
use serde::Serialize;
use std::path::PathBuf;
use tower_sessions::Session;

use crate::{auth::get_current_user, AppState};

#[derive(Debug, Clone, Serialize)]
pub struct FileEntry {
    pub name: String,
    pub is_dir: bool,
    pub size: u64,
}

#[derive(Template)]
#[template(path = "files.html")]
pub struct FilesTemplate {
    pub user: Option<String>,
    pub channel: String,
    pub path: String,
    pub entries: Vec<FileEntry>,
    pub error: Option<String>,
}

fn validate_path(workspace: &str, channel: &str, subpath: &str) -> Result<PathBuf> {
    let base = PathBuf::from(workspace).join(channel);
    let full = base.join(subpath);

    let canonical_base = base.canonicalize().unwrap_or(base.clone());
    let canonical_full = full.canonicalize().unwrap_or(full.clone());

    if !canonical_full.starts_with(&canonical_base) {
        bail!("Path traversal detected");
    }

    Ok(full)
}

pub async fn list_files(
    State(state): State<AppState>,
    session: Session,
    Path((channel, path)): Path<(String, String)>,
) -> impl IntoResponse {
    let user = get_current_user(&session).await;

    if user.is_none() {
        return Html(
            FilesTemplate {
                user: None,
                channel,
                path,
                entries: vec![],
                error: Some("Not authenticated".to_string()),
            }
            .render()
            .unwrap(),
        );
    }

    let validated = match validate_path(&state.config.workspace_path, &channel, &path) {
        Ok(p) => p,
        Err(e) => {
            return Html(
                FilesTemplate {
                    user,
                    channel,
                    path,
                    entries: vec![],
                    error: Some(e.to_string()),
                }
                .render()
                .unwrap(),
            );
        }
    };

    let entries = match std::fs::read_dir(&validated) {
        Ok(dir) => dir
            .filter_map(|e| e.ok())
            .map(|e| {
                let metadata = e.metadata().ok();
                FileEntry {
                    name: e.file_name().to_string_lossy().to_string(),
                    is_dir: e.file_type().map(|t| t.is_dir()).unwrap_or(false),
                    size: metadata.map(|m| m.len()).unwrap_or(0),
                }
            })
            .collect(),
        Err(e) => {
            return Html(
                FilesTemplate {
                    user,
                    channel,
                    path,
                    entries: vec![],
                    error: Some(e.to_string()),
                }
                .render()
                .unwrap(),
            );
        }
    };

    Html(
        FilesTemplate {
            user,
            channel,
            path,
            entries,
            error: None,
        }
        .render()
        .unwrap(),
    )
}
