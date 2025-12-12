// ABOUTME: Persistent session storage for Matrix room conversations using SQLite database.
// ABOUTME: Maps channel names to Claude sessions backed by workspace directories.
use anyhow::{Context, Result};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// Recursively copy all contents from source directory to destination
fn copy_dir_contents(src: &Path, dst: &Path) -> Result<()> {
    for entry in std::fs::read_dir(src).context("Failed to read template directory")? {
        let entry = entry.context("Failed to read directory entry")?;
        let file_type = entry.file_type().context("Failed to get file type")?;
        let src_path = entry.path();
        let file_name = entry.file_name();
        let dst_path = dst.join(&file_name);

        if file_type.is_dir() {
            std::fs::create_dir_all(&dst_path)
                .with_context(|| format!("Failed to create directory: {}", dst_path.display()))?;
            copy_dir_contents(&src_path, &dst_path)?;
        } else {
            std::fs::copy(&src_path, &dst_path).with_context(|| {
                format!(
                    "Failed to copy file from {} to {}",
                    src_path.display(),
                    dst_path.display()
                )
            })?;
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Channel {
    pub channel_name: String,
    pub room_id: String,
    pub session_id: String,
    pub directory: String,
    pub started: bool,
    pub created_at: String,
}

impl Channel {
    pub fn cli_args(&self) -> Vec<&str> {
        if self.started {
            vec!["--resume", &self.session_id]
        } else {
            vec!["--session-id", &self.session_id]
        }
    }

    /// Validate that the channel directory path is safe (no path traversal)
    /// This guards against tampered database entries
    pub fn validate_directory(&self) -> Result<()> {
        // Reject any path containing ".." to prevent path traversal
        if self.directory.contains("..") {
            tracing::error!(
                channel = %self.channel_name,
                directory = %self.directory,
                "Channel has invalid directory path (contains ..)"
            );
            anyhow::bail!("Invalid channel directory: contains path traversal");
        }
        Ok(())
    }
}

#[derive(Clone)]
pub struct SessionStore {
    db: Arc<Mutex<Connection>>,
    workspace_path: PathBuf,
}

impl SessionStore {
    pub fn new<P: AsRef<Path>>(workspace_path: P) -> Result<Self> {
        let workspace_path = workspace_path.as_ref().to_path_buf();

        // Create workspace directory if it doesn't exist
        std::fs::create_dir_all(&workspace_path).context("Failed to create workspace directory")?;

        let db_path = workspace_path.join("sessions.db");
        let conn = Connection::open(&db_path).context("Failed to open SQLite database")?;

        // Create channels table
        conn.execute(
            "CREATE TABLE IF NOT EXISTS channels (
                channel_name TEXT PRIMARY KEY,
                room_id TEXT NOT NULL UNIQUE,
                session_id TEXT NOT NULL,
                directory TEXT NOT NULL,
                started INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL
            )",
            [],
        )?;

        // Create settings table for storing app state like last-used prefix
        conn.execute(
            "CREATE TABLE IF NOT EXISTS settings (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            )",
            [],
        )?;

        tracing::info!(
            workspace = %workspace_path.display(),
            db = %db_path.display(),
            "SessionStore initialized"
        );

        Ok(SessionStore {
            db: Arc::new(Mutex::new(conn)),
            workspace_path,
        })
    }

    /// Get the shared database connection for use by other stores (like SchedulerStore)
    pub fn db_connection(&self) -> Arc<Mutex<Connection>> {
        Arc::clone(&self.db)
    }

    /// Get channel by room ID
    pub fn get_by_room(&self, room_id: &str) -> Result<Option<Channel>> {
        let db = self.db.lock().unwrap();
        let mut stmt = db.prepare(
            "SELECT channel_name, room_id, session_id, directory, started, created_at
             FROM channels WHERE room_id = ?1",
        )?;

        let channel = stmt.query_row(params![room_id], |row| {
            Ok(Channel {
                channel_name: row.get(0)?,
                room_id: row.get(1)?,
                session_id: row.get(2)?,
                directory: row.get(3)?,
                started: row.get::<_, i32>(4)? != 0,
                created_at: row.get(5)?,
            })
        });

        match channel {
            Ok(c) => {
                c.validate_directory()?;
                Ok(Some(c))
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Get channel by name (case-insensitive)
    pub fn get_by_name(&self, channel_name: &str) -> Result<Option<Channel>> {
        // Normalize to lowercase for case-insensitive lookup
        let channel_name = channel_name.to_lowercase();

        let db = self.db.lock().unwrap();
        let mut stmt = db.prepare(
            "SELECT channel_name, room_id, session_id, directory, started, created_at
             FROM channels WHERE channel_name = ?1",
        )?;

        let channel = stmt.query_row(params![channel_name], |row| {
            Ok(Channel {
                channel_name: row.get(0)?,
                room_id: row.get(1)?,
                session_id: row.get(2)?,
                directory: row.get(3)?,
                started: row.get::<_, i32>(4)? != 0,
                created_at: row.get(5)?,
            })
        });

        match channel {
            Ok(c) => {
                c.validate_directory()?;
                Ok(Some(c))
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Create a new channel with auto-generated session ID and directory
    pub fn create_channel(&self, channel_name: &str, room_id: &str) -> Result<Channel> {
        // Normalize to lowercase for case-insensitive matching
        let channel_name = channel_name.to_lowercase();

        // Validate channel_name
        if !channel_name
            .chars()
            .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
        {
            anyhow::bail!("Invalid channel name: must be alphanumeric with dashes/underscores");
        }
        if channel_name.is_empty() || channel_name.len() > 64 {
            anyhow::bail!("Channel name must be 1-64 characters");
        }
        if channel_name.starts_with('.') || channel_name.starts_with('-') {
            anyhow::bail!("Channel name cannot start with . or -");
        }

        let channel_dir = self.workspace_path.join(&channel_name);

        let channel = Channel {
            channel_name: channel_name.clone(),
            room_id: room_id.to_string(),
            session_id: uuid::Uuid::new_v4().to_string(),
            directory: channel_dir.to_string_lossy().to_string(),
            started: false,
            created_at: chrono::Utc::now().to_rfc3339(),
        };

        // Try database insert first (prevents race condition)
        let db = self
            .db
            .lock()
            .map_err(|e| anyhow::anyhow!("Database lock poisoned: {}", e))?;

        match db.execute(
            "INSERT INTO channels (channel_name, room_id, session_id, directory, started, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                &channel.channel_name,
                &channel.room_id,
                &channel.session_id,
                &channel.directory,
                if channel.started { 1 } else { 0 },
                &channel.created_at,
            ],
        ) {
            Ok(_) => {
                // Release lock before file I/O
                drop(db);

                // Check if directory already exists (inherit existing workspace)
                let dir_existed = channel_dir.exists();

                if dir_existed {
                    tracing::info!(
                        channel_name = %channel_name,
                        directory = %channel_dir.display(),
                        "Inheriting existing workspace directory"
                    );
                } else {
                    // Create directory only if it doesn't exist
                    std::fs::create_dir_all(&channel_dir)
                        .context("Failed to create channel directory")?;

                    // Copy template directory if it exists
                    let template_dir = self.workspace_path.join("template");
                    if template_dir.exists() && template_dir.is_dir() {
                        copy_dir_contents(&template_dir, &channel_dir)
                            .context("Failed to copy template directory contents")?;
                        tracing::info!(
                            template = %template_dir.display(),
                            destination = %channel_dir.display(),
                            "Copied template to new channel"
                        );
                    }
                }

                tracing::info!(
                    channel_name = %channel_name,
                    room_id = %room_id,
                    session_id = %channel.session_id,
                    directory = %channel.directory,
                    inherited = dir_existed,
                    "Channel created"
                );

                Ok(channel)
            }
            Err(e) => {
                if let rusqlite::Error::SqliteFailure(sqlite_err, _) = &e {
                    if sqlite_err.code == rusqlite::ErrorCode::ConstraintViolation {
                        anyhow::bail!("Channel name or room already exists");
                    }
                }
                Err(e.into())
            }
        }
    }

    /// List all channels
    pub fn list_all(&self) -> Result<Vec<Channel>> {
        let db = self.db.lock().unwrap();
        let mut stmt = db.prepare(
            "SELECT channel_name, room_id, session_id, directory, started, created_at
             FROM channels ORDER BY created_at DESC",
        )?;

        let channels = stmt
            .query_map([], |row| {
                Ok(Channel {
                    channel_name: row.get(0)?,
                    room_id: row.get(1)?,
                    session_id: row.get(2)?,
                    directory: row.get(3)?,
                    started: row.get::<_, i32>(4)? != 0,
                    created_at: row.get(5)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(channels)
    }

    /// Delete a channel by name
    pub fn delete_channel(&self, channel_name: &str) -> Result<()> {
        let db = self.db.lock().unwrap();
        db.execute(
            "DELETE FROM channels WHERE channel_name = ?1",
            params![channel_name],
        )?;

        tracing::info!(channel_name = %channel_name, "Channel deleted");
        Ok(())
    }

    /// Delete a channel by room ID
    pub fn delete_by_room(&self, room_id: &str) -> Result<Option<String>> {
        // Get channel name first for logging
        let channel_name = {
            let db = self.db.lock().unwrap();
            let mut stmt = db.prepare("SELECT channel_name FROM channels WHERE room_id = ?1")?;
            stmt.query_row(params![room_id], |row| row.get::<_, String>(0))
                .ok()
        };

        if let Some(ref name) = channel_name {
            let db = self.db.lock().unwrap();
            db.execute("DELETE FROM channels WHERE room_id = ?1", params![room_id])?;
            tracing::info!(channel_name = %name, room_id = %room_id, "Channel deleted by room ID");
        }

        Ok(channel_name)
    }

    /// Mark channel as started
    pub fn mark_started(&self, room_id: &str) -> Result<()> {
        let db = self.db.lock().unwrap();
        db.execute(
            "UPDATE channels SET started = 1 WHERE room_id = ?1",
            params![room_id],
        )?;
        Ok(())
    }

    /// Get channel by session ID (for webhook lookups)
    pub fn get_by_session_id(&self, session_id: &str) -> Result<Option<Channel>> {
        let db = self.db.lock().unwrap();
        let mut stmt = db.prepare(
            "SELECT channel_name, room_id, session_id, directory, started, created_at
             FROM channels WHERE session_id = ?1",
        )?;

        let channel = stmt.query_row(params![session_id], |row| {
            Ok(Channel {
                channel_name: row.get(0)?,
                room_id: row.get(1)?,
                session_id: row.get(2)?,
                directory: row.get(3)?,
                started: row.get::<_, i32>(4)? != 0,
                created_at: row.get(5)?,
            })
        });

        match channel {
            Ok(c) => {
                c.validate_directory()?;
                Ok(Some(c))
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Get a setting value by key
    pub fn get_setting(&self, key: &str) -> Result<Option<String>> {
        let db = self.db.lock().unwrap();
        let mut stmt = db.prepare("SELECT value FROM settings WHERE key = ?1")?;
        let value = stmt.query_row(params![key], |row| row.get::<_, String>(0));

        match value {
            Ok(v) => Ok(Some(v)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Set a setting value (upserts)
    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        let db = self.db.lock().unwrap();
        db.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = ?2",
            params![key, value],
        )?;
        Ok(())
    }
}
