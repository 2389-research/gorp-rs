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
    /// Optional backend type override (e.g., "acp", "mux", "direct")
    /// If None, uses the global default from config
    pub backend_type: Option<String>,
    /// True if this is the DISPATCH control plane room (1:1 DM)
    pub is_dispatch_room: bool,
}

/// An event from a worker room routed to DISPATCH
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DispatchEvent {
    pub id: String,
    pub source_room_id: String,
    pub event_type: String,
    pub payload: serde_json::Value,
    pub created_at: String,
    pub acknowledged_at: Option<String>,
}

/// Status of a dispatched task
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DispatchTaskStatus {
    Pending,
    InProgress,
    Completed,
    Failed,
}

impl std::fmt::Display for DispatchTaskStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Pending => write!(f, "pending"),
            Self::InProgress => write!(f, "in_progress"),
            Self::Completed => write!(f, "completed"),
            Self::Failed => write!(f, "failed"),
        }
    }
}

impl std::str::FromStr for DispatchTaskStatus {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "pending" => Ok(Self::Pending),
            "in_progress" => Ok(Self::InProgress),
            "completed" => Ok(Self::Completed),
            "failed" => Ok(Self::Failed),
            _ => anyhow::bail!("Unknown task status: {}", s),
        }
    }
}

/// A task dispatched from DISPATCH to a worker room
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DispatchTask {
    pub id: String,
    pub target_room_id: String,
    pub prompt: String,
    pub status: DispatchTaskStatus,
    pub created_at: String,
    pub completed_at: Option<String>,
    pub result_summary: Option<String>,
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
                created_at TEXT NOT NULL,
                backend_type TEXT
            )",
            [],
        )?;

        // Migration: Add backend_type column if it doesn't exist (for existing databases)
        let _ = conn.execute("ALTER TABLE channels ADD COLUMN backend_type TEXT", []);

        // Migration: Add is_dispatch_room column for control plane detection
        let _ = conn.execute(
            "ALTER TABLE channels ADD COLUMN is_dispatch_room INTEGER DEFAULT 0",
            [],
        );

        // Create settings table for storing app state like last-used prefix
        conn.execute(
            "CREATE TABLE IF NOT EXISTS settings (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            )",
            [],
        )?;

        // Create mux_sessions table for mux backend message history persistence
        conn.execute(
            "CREATE TABLE IF NOT EXISTS mux_sessions (
                session_id TEXT PRIMARY KEY,
                messages_json TEXT NOT NULL,
                system_prompt TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            )",
            [],
        )?;

        // Create dispatch_events table for tracking events routed to DISPATCH
        conn.execute(
            "CREATE TABLE IF NOT EXISTS dispatch_events (
                id TEXT PRIMARY KEY,
                source_room_id TEXT NOT NULL,
                event_type TEXT NOT NULL,
                payload TEXT NOT NULL,
                created_at TEXT NOT NULL,
                acknowledged_at TEXT
            )",
            [],
        )?;

        // Create dispatch_tasks table for tracking dispatched work
        conn.execute(
            "CREATE TABLE IF NOT EXISTS dispatch_tasks (
                id TEXT PRIMARY KEY,
                target_room_id TEXT NOT NULL,
                prompt TEXT NOT NULL,
                status TEXT NOT NULL,
                created_at TEXT NOT NULL,
                completed_at TEXT,
                result_summary TEXT
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
        let db = self
            .db
            .lock()
            .map_err(|e| anyhow::anyhow!("Database mutex poisoned: {}", e))?;
        let mut stmt = db.prepare(
            "SELECT channel_name, room_id, session_id, directory, started, created_at, backend_type, is_dispatch_room
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
                backend_type: row.get(6)?,
                is_dispatch_room: row.get::<_, i32>(7)? != 0,
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

        let db = self
            .db
            .lock()
            .map_err(|e| anyhow::anyhow!("Database mutex poisoned: {}", e))?;
        let mut stmt = db.prepare(
            "SELECT channel_name, room_id, session_id, directory, started, created_at, backend_type, is_dispatch_room
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
                backend_type: row.get(6)?,
                is_dispatch_room: row.get::<_, i32>(7)? != 0,
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
            backend_type: None, // Use global default
            is_dispatch_room: false,
        };

        // Try database insert first (prevents race condition)
        let db = self
            .db
            .lock()
            .map_err(|e| anyhow::anyhow!("Database lock poisoned: {}", e))?;

        match db.execute(
            "INSERT INTO channels (channel_name, room_id, session_id, directory, started, created_at, backend_type, is_dispatch_room)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                &channel.channel_name,
                &channel.room_id,
                &channel.session_id,
                &channel.directory,
                if channel.started { 1 } else { 0 },
                &channel.created_at,
                &channel.backend_type,
                0, // is_dispatch_room defaults to false
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
        let db = self
            .db
            .lock()
            .map_err(|e| anyhow::anyhow!("Database mutex poisoned: {}", e))?;
        let mut stmt = db.prepare(
            "SELECT channel_name, room_id, session_id, directory, started, created_at, backend_type, is_dispatch_room
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
                    backend_type: row.get(6)?,
                    is_dispatch_room: row.get::<_, i32>(7)? != 0,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(channels)
    }

    /// Delete a channel by name
    pub fn delete_channel(&self, channel_name: &str) -> Result<()> {
        let db = self
            .db
            .lock()
            .map_err(|e| anyhow::anyhow!("Database mutex poisoned: {}", e))?;
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
            let db = self
                .db
                .lock()
                .map_err(|e| anyhow::anyhow!("Database mutex poisoned: {}", e))?;
            let mut stmt = db.prepare("SELECT channel_name FROM channels WHERE room_id = ?1")?;
            stmt.query_row(params![room_id], |row| row.get::<_, String>(0))
                .ok()
        };

        if let Some(ref name) = channel_name {
            let db = self
                .db
                .lock()
                .map_err(|e| anyhow::anyhow!("Database mutex poisoned: {}", e))?;
            db.execute("DELETE FROM channels WHERE room_id = ?1", params![room_id])?;
            tracing::info!(channel_name = %name, room_id = %room_id, "Channel deleted by room ID");
        }

        Ok(channel_name)
    }

    /// Mark channel as started
    pub fn mark_started(&self, room_id: &str) -> Result<()> {
        let db = self
            .db
            .lock()
            .map_err(|e| anyhow::anyhow!("Database mutex poisoned: {}", e))?;
        db.execute(
            "UPDATE channels SET started = 1 WHERE room_id = ?1",
            params![room_id],
        )?;
        Ok(())
    }

    /// Reset an orphaned session (generates new session_id and sets started = false)
    /// Use this when a session becomes orphaned (e.g., Claude CLI loses conversation data)
    pub fn reset_orphaned_session(&self, room_id: &str) -> Result<()> {
        let new_session_id = uuid::Uuid::new_v4().to_string();
        let db = self
            .db
            .lock()
            .map_err(|e| anyhow::anyhow!("Database mutex poisoned: {}", e))?;
        db.execute(
            "UPDATE channels SET session_id = ?1, started = 0 WHERE room_id = ?2",
            params![new_session_id, room_id],
        )?;
        tracing::info!(
            room_id = %room_id,
            new_session_id = %new_session_id,
            "Session reset due to orphaned conversation"
        );
        Ok(())
    }

    /// Get channel by session ID (for webhook lookups)
    pub fn get_by_session_id(&self, session_id: &str) -> Result<Option<Channel>> {
        let db = self
            .db
            .lock()
            .map_err(|e| anyhow::anyhow!("Database mutex poisoned: {}", e))?;
        let mut stmt = db.prepare(
            "SELECT channel_name, room_id, session_id, directory, started, created_at, backend_type, is_dispatch_room
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
                backend_type: row.get(6)?,
                is_dispatch_room: row.get::<_, i32>(7)? != 0,
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
        let db = self
            .db
            .lock()
            .map_err(|e| anyhow::anyhow!("Database mutex poisoned: {}", e))?;
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
        let db = self
            .db
            .lock()
            .map_err(|e| anyhow::anyhow!("Database mutex poisoned: {}", e))?;
        db.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = ?2",
            params![key, value],
        )?;
        Ok(())
    }

    /// Reset a channel's session (new session ID and started=0)
    pub fn reset_session(&self, channel_name: &str, new_session_id: &str) -> Result<()> {
        let db = self
            .db
            .lock()
            .map_err(|e| anyhow::anyhow!("Database mutex poisoned: {}", e))?;
        db.execute(
            "UPDATE channels SET session_id = ?1, started = 0 WHERE channel_name = ?2",
            params![new_session_id, channel_name],
        )?;
        tracing::info!(
            channel_name = %channel_name,
            new_session_id = %new_session_id,
            "Channel session reset"
        );
        Ok(())
    }

    /// Update backend type for a channel
    /// Pass None to reset to global default
    pub fn update_backend_type(
        &self,
        channel_name: &str,
        backend_type: Option<&str>,
    ) -> Result<()> {
        let db = self
            .db
            .lock()
            .map_err(|e| anyhow::anyhow!("Database mutex poisoned: {}", e))?;
        db.execute(
            "UPDATE channels SET backend_type = ?1 WHERE channel_name = ?2",
            params![backend_type, channel_name],
        )?;
        tracing::info!(
            channel_name = %channel_name,
            backend_type = ?backend_type,
            "Channel backend type updated"
        );
        Ok(())
    }

    /// Update session ID for a channel by room ID (used when new session is created)
    pub fn update_session_id(&self, room_id: &str, new_session_id: &str) -> Result<()> {
        let db = self
            .db
            .lock()
            .map_err(|e| anyhow::anyhow!("Database mutex poisoned: {}", e))?;
        db.execute(
            "UPDATE channels SET session_id = ?1 WHERE room_id = ?2",
            params![new_session_id, room_id],
        )?;
        tracing::debug!(
            room_id = %room_id,
            new_session_id = %new_session_id,
            "Session ID updated"
        );
        Ok(())
    }

    // =========================================================================
    // DISPATCH Channel Methods
    // =========================================================================

    /// Get the DISPATCH channel for a room (if it exists)
    pub fn get_dispatch_channel(&self, room_id: &str) -> Result<Option<Channel>> {
        let db = self
            .db
            .lock()
            .map_err(|e| anyhow::anyhow!("Database mutex poisoned: {}", e))?;
        let mut stmt = db.prepare(
            "SELECT channel_name, room_id, session_id, directory, started, created_at, backend_type, is_dispatch_room
             FROM channels WHERE room_id = ?1 AND is_dispatch_room = 1",
        )?;

        let channel = stmt.query_row(params![room_id], |row| {
            Ok(Channel {
                channel_name: row.get(0)?,
                room_id: row.get(1)?,
                session_id: row.get(2)?,
                directory: row.get(3)?,
                started: row.get::<_, i32>(4)? != 0,
                created_at: row.get(5)?,
                backend_type: row.get(6)?,
                is_dispatch_room: row.get::<_, i32>(7)? != 0,
            })
        });

        match channel {
            Ok(c) => Ok(Some(c)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Create a DISPATCH channel for a room (control plane, no workspace)
    ///
    /// Unlike regular channels, DISPATCH channels:
    /// - Have is_dispatch_room = true
    /// - Have an empty directory (no filesystem workspace)
    /// - Use channel_name = "dispatch:{room_id}" for uniqueness
    pub fn create_dispatch_channel(&self, room_id: &str) -> Result<Channel> {
        let channel = Channel {
            channel_name: format!("dispatch:{}", room_id),
            room_id: room_id.to_string(),
            session_id: uuid::Uuid::new_v4().to_string(),
            directory: String::new(), // No workspace for DISPATCH
            started: false,
            created_at: chrono::Utc::now().to_rfc3339(),
            backend_type: Some("mux".to_string()), // DISPATCH always uses mux
            is_dispatch_room: true,
        };

        let db = self
            .db
            .lock()
            .map_err(|e| anyhow::anyhow!("Database lock poisoned: {}", e))?;

        db.execute(
            "INSERT INTO channels (channel_name, room_id, session_id, directory, started, created_at, backend_type, is_dispatch_room)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                &channel.channel_name,
                &channel.room_id,
                &channel.session_id,
                &channel.directory,
                if channel.started { 1 } else { 0 },
                &channel.created_at,
                &channel.backend_type,
                if channel.is_dispatch_room { 1 } else { 0 },
            ],
        )?;

        tracing::info!(
            room_id = %room_id,
            session_id = %channel.session_id,
            "DISPATCH channel created"
        );

        Ok(channel)
    }

    /// Get existing DISPATCH channel or create one for this room
    pub fn get_or_create_dispatch_channel(&self, room_id: &str) -> Result<Channel> {
        if let Some(channel) = self.get_dispatch_channel(room_id)? {
            return Ok(channel);
        }
        self.create_dispatch_channel(room_id)
    }

    /// List all DISPATCH channels (for startup notifications)
    pub fn list_dispatch_channels(&self) -> Result<Vec<Channel>> {
        let db = self
            .db
            .lock()
            .map_err(|e| anyhow::anyhow!("Database mutex poisoned: {}", e))?;

        let mut stmt = db.prepare(
            "SELECT channel_name, room_id, session_id, directory, started, created_at, backend_type, is_dispatch_room
             FROM channels WHERE is_dispatch_room = 1",
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
                    backend_type: row.get(6)?,
                    is_dispatch_room: row.get::<_, i32>(7)? != 0,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();

        Ok(channels)
    }

    /// Get onboarding state for a user (stored as JSON in settings table)
    pub fn get_onboarding_state(&self, user_id: &str) -> Result<Option<String>> {
        let key = format!("onboarding:{}", user_id);
        self.get_setting(&key)
    }

    /// Set onboarding state for a user (stored as JSON in settings table)
    pub fn set_onboarding_state(&self, user_id: &str, state_json: &str) -> Result<()> {
        let key = format!("onboarding:{}", user_id);
        self.set_setting(&key, state_json)
    }

    /// Clear onboarding state for a user
    pub fn clear_onboarding_state(&self, user_id: &str) -> Result<()> {
        let key = format!("onboarding:{}", user_id);
        let db = self
            .db
            .lock()
            .map_err(|e| anyhow::anyhow!("Database mutex poisoned: {}", e))?;
        db.execute("DELETE FROM settings WHERE key = ?1", params![key])?;
        Ok(())
    }

    // =========================================================================
    // Mux Session Persistence
    // =========================================================================

    /// Save a mux session's message history to the database
    pub fn save_mux_session(
        &self,
        session_id: &str,
        messages_json: &str,
        system_prompt: Option<&str>,
    ) -> Result<()> {
        let db = self
            .db
            .lock()
            .map_err(|e| anyhow::anyhow!("Database mutex poisoned: {}", e))?;

        let now = chrono::Utc::now().to_rfc3339();

        db.execute(
            "INSERT INTO mux_sessions (session_id, messages_json, system_prompt, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?4)
             ON CONFLICT(session_id) DO UPDATE SET
                messages_json = ?2,
                system_prompt = ?3,
                updated_at = ?4",
            params![session_id, messages_json, system_prompt, now],
        )?;

        tracing::debug!(session_id = %session_id, "Mux session saved");
        Ok(())
    }

    /// Load a mux session's message history from the database
    /// Returns (messages_json, system_prompt) if found
    pub fn load_mux_session(&self, session_id: &str) -> Result<Option<(String, Option<String>)>> {
        let db = self
            .db
            .lock()
            .map_err(|e| anyhow::anyhow!("Database mutex poisoned: {}", e))?;

        let mut stmt = db.prepare(
            "SELECT messages_json, system_prompt FROM mux_sessions WHERE session_id = ?1",
        )?;

        let result = stmt.query_row(params![session_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
        });

        match result {
            Ok((messages, prompt)) => {
                tracing::debug!(session_id = %session_id, "Mux session loaded");
                Ok(Some((messages, prompt)))
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Delete a mux session from the database
    pub fn delete_mux_session(&self, session_id: &str) -> Result<()> {
        let db = self
            .db
            .lock()
            .map_err(|e| anyhow::anyhow!("Database mutex poisoned: {}", e))?;

        db.execute(
            "DELETE FROM mux_sessions WHERE session_id = ?1",
            params![session_id],
        )?;

        tracing::debug!(session_id = %session_id, "Mux session deleted");
        Ok(())
    }

    /// Check if a mux session exists in the database
    pub fn mux_session_exists(&self, session_id: &str) -> Result<bool> {
        let db = self
            .db
            .lock()
            .map_err(|e| anyhow::anyhow!("Database mutex poisoned: {}", e))?;

        let mut stmt = db.prepare("SELECT 1 FROM mux_sessions WHERE session_id = ?1")?;
        let exists = stmt.exists(params![session_id])?;
        Ok(exists)
    }

    // =========================================================================
    // Dispatch Event Persistence
    // =========================================================================

    /// Insert a dispatch event
    pub fn insert_dispatch_event(&self, event: &DispatchEvent) -> Result<()> {
        let db = self
            .db
            .lock()
            .map_err(|e| anyhow::anyhow!("Database mutex poisoned: {}", e))?;
        let payload_str = serde_json::to_string(&event.payload)?;
        db.execute(
            "INSERT INTO dispatch_events (id, source_room_id, event_type, payload, created_at, acknowledged_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                &event.id,
                &event.source_room_id,
                &event.event_type,
                payload_str,
                &event.created_at,
                &event.acknowledged_at,
            ],
        )?;
        Ok(())
    }

    /// Get all pending (unacknowledged) dispatch events
    pub fn get_pending_dispatch_events(&self) -> Result<Vec<DispatchEvent>> {
        let db = self
            .db
            .lock()
            .map_err(|e| anyhow::anyhow!("Database mutex poisoned: {}", e))?;
        let mut stmt = db.prepare(
            "SELECT id, source_room_id, event_type, payload, created_at, acknowledged_at
             FROM dispatch_events WHERE acknowledged_at IS NULL ORDER BY created_at ASC",
        )?;

        let events = stmt
            .query_map([], |row| {
                let payload_str: String = row.get(3)?;
                let payload: serde_json::Value =
                    serde_json::from_str(&payload_str).map_err(|e| {
                        rusqlite::Error::FromSqlConversionFailure(
                            3,
                            rusqlite::types::Type::Text,
                            Box::new(e),
                        )
                    })?;
                Ok(DispatchEvent {
                    id: row.get(0)?,
                    source_room_id: row.get(1)?,
                    event_type: row.get(2)?,
                    payload,
                    created_at: row.get(4)?,
                    acknowledged_at: row.get(5)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(events)
    }

    /// Acknowledge a dispatch event (marks it as processed)
    pub fn acknowledge_dispatch_event(&self, id: &str) -> Result<()> {
        let db = self
            .db
            .lock()
            .map_err(|e| anyhow::anyhow!("Database mutex poisoned: {}", e))?;
        let now = chrono::Utc::now().to_rfc3339();
        db.execute(
            "UPDATE dispatch_events SET acknowledged_at = ?1 WHERE id = ?2",
            params![now, id],
        )?;
        Ok(())
    }

    // =========================================================================
    // Dispatch Task Persistence
    // =========================================================================

    /// Create a new dispatch task
    pub fn create_dispatch_task(&self, target_room_id: &str, prompt: &str) -> Result<DispatchTask> {
        let task = DispatchTask {
            id: uuid::Uuid::new_v4().to_string(),
            target_room_id: target_room_id.to_string(),
            prompt: prompt.to_string(),
            status: DispatchTaskStatus::Pending,
            created_at: chrono::Utc::now().to_rfc3339(),
            completed_at: None,
            result_summary: None,
        };

        let db = self
            .db
            .lock()
            .map_err(|e| anyhow::anyhow!("Database mutex poisoned: {}", e))?;
        db.execute(
            "INSERT INTO dispatch_tasks (id, target_room_id, prompt, status, created_at, completed_at, result_summary)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                &task.id,
                &task.target_room_id,
                &task.prompt,
                task.status.to_string(),
                &task.created_at,
                &task.completed_at,
                &task.result_summary,
            ],
        )?;

        Ok(task)
    }

    /// Get a dispatch task by ID
    pub fn get_dispatch_task(&self, id: &str) -> Result<Option<DispatchTask>> {
        let db = self
            .db
            .lock()
            .map_err(|e| anyhow::anyhow!("Database mutex poisoned: {}", e))?;
        let mut stmt = db.prepare(
            "SELECT id, target_room_id, prompt, status, created_at, completed_at, result_summary
             FROM dispatch_tasks WHERE id = ?1",
        )?;

        let task = stmt.query_row(params![id], |row| {
            let status_str: String = row.get(3)?;
            let status: DispatchTaskStatus = match status_str.as_str() {
                "pending" => DispatchTaskStatus::Pending,
                "in_progress" => DispatchTaskStatus::InProgress,
                "completed" => DispatchTaskStatus::Completed,
                "failed" => DispatchTaskStatus::Failed,
                _ => {
                    return Err(rusqlite::Error::FromSqlConversionFailure(
                        3,
                        rusqlite::types::Type::Text,
                        format!("Unknown task status: {}", status_str).into(),
                    ))
                }
            };
            Ok(DispatchTask {
                id: row.get(0)?,
                target_room_id: row.get(1)?,
                prompt: row.get(2)?,
                status,
                created_at: row.get(4)?,
                completed_at: row.get(5)?,
                result_summary: row.get(6)?,
            })
        });

        match task {
            Ok(t) => Ok(Some(t)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Update a dispatch task's status
    pub fn update_dispatch_task_status(
        &self,
        id: &str,
        status: DispatchTaskStatus,
        result_summary: Option<&str>,
    ) -> Result<()> {
        let db = self
            .db
            .lock()
            .map_err(|e| anyhow::anyhow!("Database mutex poisoned: {}", e))?;

        let completed_at = if matches!(
            status,
            DispatchTaskStatus::Completed | DispatchTaskStatus::Failed
        ) {
            Some(chrono::Utc::now().to_rfc3339())
        } else {
            None
        };

        db.execute(
            "UPDATE dispatch_tasks SET status = ?1, completed_at = ?2, result_summary = ?3 WHERE id = ?4",
            params![status.to_string(), completed_at, result_summary, id],
        )?;

        Ok(())
    }

    /// List dispatch tasks, optionally filtered by status
    pub fn list_dispatch_tasks(
        &self,
        status: Option<DispatchTaskStatus>,
    ) -> Result<Vec<DispatchTask>> {
        let db = self
            .db
            .lock()
            .map_err(|e| anyhow::anyhow!("Database mutex poisoned: {}", e))?;

        // Helper to parse a row into DispatchTask
        fn parse_task_row(row: &rusqlite::Row) -> rusqlite::Result<DispatchTask> {
            let status_str: String = row.get(3)?;
            let status = match status_str.as_str() {
                "pending" => DispatchTaskStatus::Pending,
                "in_progress" => DispatchTaskStatus::InProgress,
                "completed" => DispatchTaskStatus::Completed,
                "failed" => DispatchTaskStatus::Failed,
                _ => {
                    return Err(rusqlite::Error::FromSqlConversionFailure(
                        3,
                        rusqlite::types::Type::Text,
                        format!("Unknown task status: {}", status_str).into(),
                    ))
                }
            };
            Ok(DispatchTask {
                id: row.get(0)?,
                target_room_id: row.get(1)?,
                prompt: row.get(2)?,
                status,
                created_at: row.get(4)?,
                completed_at: row.get(5)?,
                result_summary: row.get(6)?,
            })
        }

        let tasks = match status {
            Some(s) => {
                let mut stmt = db.prepare(
                    "SELECT id, target_room_id, prompt, status, created_at, completed_at, result_summary
                     FROM dispatch_tasks WHERE status = ?1 ORDER BY created_at DESC",
                )?;
                let result = stmt
                    .query_map(params![s.to_string()], parse_task_row)?
                    .collect::<Result<Vec<_>, _>>()?;
                result
            }
            None => {
                let mut stmt = db.prepare(
                    "SELECT id, target_room_id, prompt, status, created_at, completed_at, result_summary
                     FROM dispatch_tasks ORDER BY created_at DESC",
                )?;
                let result = stmt
                    .query_map([], parse_task_row)?
                    .collect::<Result<Vec<_>, _>>()?;
                result
            }
        };

        Ok(tasks)
    }
}
