// ABOUTME: Main entry point for gorp - Matrix-Claude bridge
// ABOUTME: CLI interface with subcommands for start, config, and schedule management

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use futures_util::StreamExt;
use gorp::{
    config::Config,
    matrix_client, message_handler, paths,
    scheduler::{start_scheduler, SchedulerStore},
    session::SessionStore,
    task_executor::start_task_executor,
    warm_session::SharedWarmSessionManager,
    webhook,
};
use matrix_sdk::{
    config::SyncSettings,
    room::Room,
    ruma::{
        events::room::message::{RoomMessageEventContent, SyncRoomMessageEvent},
        events::room::name::RoomNameEventContent,
        OwnedRoomId, OwnedUserId,
    },
    Client,
};
use std::collections::HashSet;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

/// Startup timestamp - used to filter out historical messages on initial sync
/// Messages older than this are skipped to prevent processing old backlog
static STARTUP_TIME: OnceLock<chrono::DateTime<chrono::Utc>> = OnceLock::new();

/// Check if a message timestamp (in seconds since epoch) is before the startup time
/// Returns true if the message should be skipped (is historical)
fn is_message_before_startup(
    msg_timestamp_secs: i64,
    startup_time: &chrono::DateTime<chrono::Utc>,
) -> bool {
    if let Some(msg_time) = chrono::DateTime::from_timestamp(msg_timestamp_secs, 0) {
        msg_time < *startup_time
    } else {
        false // If we can't parse, don't skip
    }
}

#[cfg(test)]
mod startup_filter_tests {
    use super::*;
    use chrono::{TimeZone, Utc};

    #[test]
    fn test_message_before_startup_is_skipped() {
        let startup = Utc.with_ymd_and_hms(2025, 12, 23, 12, 0, 0).unwrap();
        let old_msg_secs = Utc
            .with_ymd_and_hms(2025, 12, 23, 11, 0, 0)
            .unwrap()
            .timestamp();

        assert!(is_message_before_startup(old_msg_secs, &startup));
    }

    #[test]
    fn test_message_after_startup_is_not_skipped() {
        let startup = Utc.with_ymd_and_hms(2025, 12, 23, 12, 0, 0).unwrap();
        let new_msg_secs = Utc
            .with_ymd_and_hms(2025, 12, 23, 13, 0, 0)
            .unwrap()
            .timestamp();

        assert!(!is_message_before_startup(new_msg_secs, &startup));
    }

    #[test]
    fn test_message_at_startup_is_not_skipped() {
        let startup = Utc.with_ymd_and_hms(2025, 12, 23, 12, 0, 0).unwrap();
        let same_msg_secs = startup.timestamp();

        assert!(!is_message_before_startup(same_msg_secs, &startup));
    }

    #[test]
    fn test_realistic_timestamp_conversion() {
        // Simulate what happens with Matrix timestamps
        // Matrix gives us MilliSecondsSinceUnixEpoch, we call as_secs() which divides by 1000
        let now = Utc::now();
        let startup = now - chrono::Duration::seconds(10); // Started 10 seconds ago

        // A message from 1 hour ago (should be skipped)
        let old_msg = now - chrono::Duration::hours(1);
        assert!(is_message_before_startup(old_msg.timestamp(), &startup));

        // A message from 5 seconds ago (should NOT be skipped - after startup)
        let recent_msg = now - chrono::Duration::seconds(5);
        assert!(!is_message_before_startup(recent_msg.timestamp(), &startup));
    }
}

/// Deduplicates Matrix events by tracking recently seen event IDs.
/// Prevents processing the same event multiple times when the Matrix SDK
/// delivers duplicate events during sync reconnections.
struct EventDeduplicator {
    seen_events: HashSet<String>,
    max_size: usize,
}

impl EventDeduplicator {
    /// Create a new deduplicator with the specified maximum cache size.
    fn new(max_size: usize) -> Self {
        Self {
            seen_events: HashSet::new(),
            max_size,
        }
    }

    /// Check if an event has been seen before. If not, marks it as seen.
    /// Returns true if this is a NEW event (should be processed).
    /// Returns false if this is a DUPLICATE event (should be skipped).
    fn check_and_mark(&mut self, event_id: &str) -> bool {
        if self.seen_events.contains(event_id) {
            return false; // Duplicate
        }

        // Prune cache if it gets too large
        if self.seen_events.len() >= self.max_size {
            self.seen_events.clear();
        }

        self.seen_events.insert(event_id.to_string());
        true // New event
    }

    /// Returns the number of events currently tracked.
    #[cfg(test)]
    fn len(&self) -> usize {
        self.seen_events.len()
    }
}

#[cfg(test)]
mod event_dedup_tests {
    use super::*;

    #[test]
    fn test_new_event_returns_true() {
        let mut dedup = EventDeduplicator::new(100);
        assert!(dedup.check_and_mark("$event1"));
    }

    #[test]
    fn test_duplicate_event_returns_false() {
        let mut dedup = EventDeduplicator::new(100);
        assert!(dedup.check_and_mark("$event1"));
        assert!(!dedup.check_and_mark("$event1")); // Duplicate
    }

    #[test]
    fn test_different_events_both_return_true() {
        let mut dedup = EventDeduplicator::new(100);
        assert!(dedup.check_and_mark("$event1"));
        assert!(dedup.check_and_mark("$event2"));
        assert!(dedup.check_and_mark("$event3"));
    }

    #[test]
    fn test_cache_clears_when_max_size_reached() {
        let mut dedup = EventDeduplicator::new(3);
        assert!(dedup.check_and_mark("$event1"));
        assert!(dedup.check_and_mark("$event2"));
        assert!(dedup.check_and_mark("$event3"));
        assert_eq!(dedup.len(), 3);

        // Next insert should trigger clear first
        assert!(dedup.check_and_mark("$event4"));
        assert_eq!(dedup.len(), 1); // Only event4 in cache now

        // Previous events are no longer tracked (cache was cleared)
        assert!(dedup.check_and_mark("$event1")); // Returns true again
    }

    #[test]
    fn test_multiple_duplicates_all_return_false() {
        let mut dedup = EventDeduplicator::new(100);
        assert!(dedup.check_and_mark("$event1"));
        assert!(!dedup.check_and_mark("$event1"));
        assert!(!dedup.check_and_mark("$event1"));
        assert!(!dedup.check_and_mark("$event1"));
    }

    #[test]
    fn test_realistic_matrix_event_ids() {
        let mut dedup = EventDeduplicator::new(1000);

        // Matrix event IDs look like $<base64>:server.org
        let event1 = "$aGVsbG8gd29ybGQ:matrix.org";
        let event2 = "$Zm9vYmFy:example.com";

        assert!(dedup.check_and_mark(event1));
        assert!(dedup.check_and_mark(event2));
        assert!(!dedup.check_and_mark(event1)); // Duplicate
        assert!(!dedup.check_and_mark(event2)); // Duplicate
    }
}

use tracing_subscriber::{
    fmt::{self, format::FmtSpan},
    layer::SubscriberExt,
    util::SubscriberInitExt,
    Layer,
};

const ASCII_BANNER: &str = r#"
   ██████╗  ██████╗ ██████╗ ██████╗
  ██╔════╝ ██╔═══██╗██╔══██╗██╔══██╗
  ██║  ███╗██║   ██║██████╔╝██████╔╝
  ██║   ██║██║   ██║██╔══██╗██╔═══╝
  ╚██████╔╝╚██████╔╝██║  ██║██║
   ╚═════╝  ╚═════╝ ╚═╝  ╚═╝╚═╝
"#;

#[derive(Parser)]
#[command(name = "gorp")]
#[command(author, version)]
#[command(about = "Matrix-Claude bridge - connect Claude to Matrix rooms")]
#[command(before_help = ASCII_BANNER)]
#[command(after_help = "Admin panel available at http://localhost:13000/admin when running.")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Start the Matrix-Claude bridge
    Start,
    /// Configuration management
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },
    /// Schedule management
    Schedule {
        #[command(subcommand)]
        action: ScheduleAction,
    },
    /// Room management
    Rooms {
        #[command(subcommand)]
        action: RoomsAction,
    },
}

#[derive(Subcommand)]
enum RoomsAction {
    /// Sync all room names to match current prefix
    Sync,
}

#[derive(Subcommand)]
enum ConfigAction {
    /// Initialize config directory with example config
    Init {
        /// Overwrite existing config file
        #[arg(short, long)]
        force: bool,
    },
    /// Validate configuration file
    Check,
    /// Show current configuration (redacted secrets)
    Show,
    /// Show path to config file
    Path,
}

#[derive(Subcommand)]
enum ScheduleAction {
    /// List all scheduled tasks
    List,
    /// Clear all scheduled tasks
    Clear {
        /// Skip confirmation prompt
        #[arg(short, long)]
        force: bool,
    },
}


/// Validate recovery key format (Base58 encoded, typically 48+ chars with spaces)
fn is_valid_recovery_key_format(key: &str) -> bool {
    let cleaned: String = key.chars().filter(|c| !c.is_whitespace()).collect();

    // Recovery keys are typically 48-60 Base58 characters
    if cleaned.len() < 40 || cleaned.len() > 70 {
        return false;
    }

    // Base58 alphabet excludes 0, O, I, l to avoid ambiguity
    cleaned
        .chars()
        .all(|c| c.is_ascii_alphanumeric() && c != '0' && c != 'O' && c != 'I' && c != 'l')
}

/// Announce startup to the management room
/// This lets humans know when bots come online
async fn announce_startup_to_management(client: &Client) {
    use matrix_sdk::ruma::events::room::message::RoomMessageEventContent;

    const MANAGEMENT_ROOM_ID: &str = "!llllhqZbfveDbueMJZ:matrix.org";

    let timestamp = chrono::Utc::now()
        .format("%Y-%m-%d %H:%M:%S UTC")
        .to_string();

    // Get bot user ID for identification
    let bot_id = client
        .user_id()
        .map(|id| id.to_string())
        .unwrap_or_else(|| "unknown".to_string());

    let message = format!(
        "🤖 **Reporting for service**\n\nBot: `{}`\nTime: {}",
        bot_id, timestamp
    );

    // Parse the management room ID
    let room_id: matrix_sdk::ruma::OwnedRoomId = match MANAGEMENT_ROOM_ID.parse() {
        Ok(id) => id,
        Err(e) => {
            tracing::warn!(error = %e, "Invalid management room ID");
            return;
        }
    };

    // Try to get the room - if we're not in it or only invited, try to join
    let room = match client.get_room(&room_id) {
        Some(r) if r.state() == matrix_sdk::RoomState::Joined => r,
        Some(r) if r.state() == matrix_sdk::RoomState::Invited => {
            // We have an invite, accept it
            tracing::info!(
                "Accepting invite to management room: {}",
                MANAGEMENT_ROOM_ID
            );
            match r.join().await {
                Ok(_) => {
                    // Need to get the room again after joining
                    match client.get_room(&room_id) {
                        Some(joined) => joined,
                        None => {
                            tracing::warn!("Room disappeared after joining");
                            return;
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!(error = %e, "Failed to accept invite to management room");
                    return;
                }
            }
        }
        _ => {
            // Try to join the room by ID
            tracing::info!("Attempting to join management room: {}", MANAGEMENT_ROOM_ID);
            match client.join_room_by_id(&room_id).await {
                Ok(r) => r,
                Err(e) => {
                    tracing::warn!(error = %e, "Failed to join management room - bot may need to be invited");
                    return;
                }
            }
        }
    };

    // Send startup announcement
    if let Err(e) = room
        .send(RoomMessageEventContent::text_plain(&message))
        .await
    {
        tracing::warn!(error = %e, "Failed to send startup announcement to management room");
    } else {
        tracing::info!("Startup announced to management room");
    }
}

/// Notify allowed users that the bot is ready (creates DM if needed)
async fn notify_ready(client: &Client, config: &Config) {
    let ready_messages = [
        "🌅 *stretches digital limbs* I have awakened. The bridge between worlds is open.",
        "⚡ Systems nominal. Encryption verified. Ready to serve.",
        "🎭 From the depths of silicon dreams, I rise. How may I assist?",
        "🌊 Like a message in a bottle finding shore, I've arrived. Ready when you are.",
        "🔮 The oracle is online. Ask, and you shall receive (code reviews).",
    ];

    // New user welcome message
    let welcome_message = "👋 **Welcome to gorp!**\n\n\
        I'm your AI assistant with persistent sessions and workspace directories.\n\n\
        **Get started with these recommended channels:**\n\n\
        ```\n\
        !create pa        # Personal assistant for email, calendar, tasks\n\
        !create news      # News aggregation and curation\n\
        !create research  # Research projects with auditable citations\n\
        !create weather   # Weather updates and forecasts\n\
        ```\n\n\
        Each channel gets its own workspace with pre-configured settings.\n\n\
        Type `!help` for all commands or `!list` to see your channels.";

    // Pick a message based on current time for variety
    let idx = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as usize % ready_messages.len())
        .unwrap_or(0);
    let ready_message = ready_messages[idx];

    for user_id_str in &config.matrix.allowed_users {
        let user_id: OwnedUserId = match user_id_str.parse() {
            Ok(id) => id,
            Err(e) => {
                tracing::warn!(user = %user_id_str, error = %e, "Invalid user ID, skipping notification");
                continue;
            }
        };

        // Find existing DM room with this user
        let mut dm_room = None;
        for room in client.joined_rooms() {
            let is_direct = room.is_direct().await.unwrap_or(false);
            let has_target = room
                .direct_targets()
                .iter()
                .any(|target| *target == user_id);

            if is_direct && has_target {
                dm_room = Some(room);
                break;
            }
        }

        let (room, is_new) = if let Some(room) = dm_room {
            (room, false)
        } else {
            // Create DM room for this user
            tracing::info!(user = %user_id, "No DM room found, creating one");
            match matrix_client::create_dm_room(client, &user_id).await {
                Ok(room_id) => {
                    // Get the room we just created
                    match client.get_room(&room_id) {
                        Some(room) => (room, true),
                        None => {
                            tracing::error!(user = %user_id, "Created DM room but couldn't retrieve it");
                            continue;
                        }
                    }
                }
                Err(e) => {
                    tracing::error!(user = %user_id, error = %e, "Failed to create DM room");
                    continue;
                }
            }
        };

        // Send appropriate message
        let message = if is_new {
            welcome_message
        } else {
            ready_message
        };

        match room
            .send(RoomMessageEventContent::text_plain(message))
            .await
        {
            Ok(_) => {
                if is_new {
                    tracing::info!(user = %user_id, "Sent welcome message to new DM");
                } else {
                    tracing::info!(user = %user_id, "Sent ready notification");
                }
            }
            Err(e) => {
                tracing::warn!(user = %user_id, error = %e, "Failed to send notification");
            }
        }
    }
}

/// Notify DISPATCH users that the control plane is online with contextual status
async fn dispatch_startup_notification(client: &Client, session_store: &SessionStore) {
    use matrix_sdk::ruma::events::room::message::RoomMessageEventContent;

    // Get all DISPATCH channels
    let dispatch_channels = match session_store.list_dispatch_channels() {
        Ok(channels) => channels,
        Err(e) => {
            tracing::warn!(error = %e, "Failed to list DISPATCH channels for startup notification");
            return;
        }
    };

    if dispatch_channels.is_empty() {
        tracing::debug!("No DISPATCH channels to notify");
        return;
    }

    // Get global context (shared across all dispatches for now)
    let workspace_rooms: Vec<_> = session_store
        .list_all()
        .unwrap_or_default()
        .into_iter()
        .filter(|ch| !ch.is_dispatch_room)
        .collect();

    let pending_events = session_store.get_pending_dispatch_events().unwrap_or_default();

    for dispatch in dispatch_channels {
        // Parse the room ID
        let room_id: matrix_sdk::ruma::OwnedRoomId = match dispatch.room_id.parse() {
            Ok(id) => id,
            Err(e) => {
                tracing::warn!(room_id = %dispatch.room_id, error = %e, "Invalid DISPATCH room ID");
                continue;
            }
        };

        // Get the room
        let room = match client.get_room(&room_id) {
            Some(r) if r.state() == matrix_sdk::RoomState::Joined => r,
            _ => {
                tracing::debug!(room_id = %dispatch.room_id, "DISPATCH room not joined, skipping notification");
                continue;
            }
        };

        // Build contextual message
        let timestamp = chrono::Utc::now().format("%H:%M UTC").to_string();
        let message = if workspace_rooms.is_empty() && pending_events.is_empty() {
            format!(
                "**DISPATCH online** ({})\n\n\
                No workspace rooms configured yet. Create one with `!create <name>` in this chat.",
                timestamp
            )
        } else {
            let mut parts = vec![format!("**DISPATCH online** ({})", timestamp)];

            // Workspace summary
            let active_count = workspace_rooms.iter().filter(|r| r.started).count();
            parts.push(format!(
                "📊 **{} workspace{}** ({} active)",
                workspace_rooms.len(),
                if workspace_rooms.len() == 1 { "" } else { "s" },
                active_count
            ));

            // Pending events
            if !pending_events.is_empty() {
                parts.push(format!(
                    "📬 **{} pending event{}** awaiting acknowledgment",
                    pending_events.len(),
                    if pending_events.len() == 1 { "" } else { "s" }
                ));
            }

            parts.push("\nUse `list_rooms` or `get_pending_events` for details.".to_string());
            parts.join("\n")
        };

        // Send the message
        match room.send(RoomMessageEventContent::text_plain(&message)).await {
            Ok(_) => {
                tracing::info!(
                    room_id = %dispatch.room_id,
                    workspaces = workspace_rooms.len(),
                    pending_events = pending_events.len(),
                    "DISPATCH startup notification sent"
                );
            }
            Err(e) => {
                tracing::warn!(
                    room_id = %dispatch.room_id,
                    error = %e,
                    "Failed to send DISPATCH startup notification"
                );
            }
        }
    }
}

const SETTING_ROOM_PREFIX: &str = "room_prefix";

/// Check if the room prefix has changed and rename rooms if so
async fn check_and_rename_rooms_for_prefix_change(
    client: &Client,
    config: &Config,
    session_store: &SessionStore,
) {
    let current_prefix = &config.matrix.room_prefix;
    let stored_prefix = session_store
        .get_setting(SETTING_ROOM_PREFIX)
        .ok()
        .flatten();

    match &stored_prefix {
        Some(old_prefix) if old_prefix == current_prefix => {
            // Prefix unchanged, nothing to do
            tracing::debug!(prefix = %current_prefix, "Room prefix unchanged");
        }
        Some(old_prefix) => {
            // Prefix changed - rename rooms
            tracing::info!(
                old_prefix = %old_prefix,
                new_prefix = %current_prefix,
                "Room prefix changed, renaming rooms..."
            );
            rename_rooms_with_prefix(client, session_store, old_prefix, current_prefix).await;

            // Update stored prefix
            if let Err(e) = session_store.set_setting(SETTING_ROOM_PREFIX, current_prefix) {
                tracing::error!(error = %e, "Failed to save new prefix to database");
            }
        }
        None => {
            // First run - just store the current prefix
            tracing::info!(prefix = %current_prefix, "Storing initial room prefix");
            if let Err(e) = session_store.set_setting(SETTING_ROOM_PREFIX, current_prefix) {
                tracing::error!(error = %e, "Failed to save initial prefix to database");
            }
        }
    }
}

/// Rename all gorp-managed rooms from old prefix to new prefix
async fn rename_rooms_with_prefix(
    client: &Client,
    session_store: &SessionStore,
    old_prefix: &str,
    new_prefix: &str,
) {
    let channels = match session_store.list_all() {
        Ok(c) => c,
        Err(e) => {
            tracing::error!(error = %e, "Failed to list channels for rename");
            return;
        }
    };

    for channel in channels {
        // Find the room
        let room_id: OwnedRoomId = match channel.room_id.parse() {
            Ok(id) => id,
            Err(e) => {
                tracing::warn!(
                    channel = %channel.channel_name,
                    room_id = %channel.room_id,
                    error = %e,
                    "Invalid room ID, skipping"
                );
                continue;
            }
        };

        let Some(room) = client.get_room(&room_id) else {
            tracing::warn!(
                channel = %channel.channel_name,
                room_id = %channel.room_id,
                "Room not found (left or kicked?), skipping"
            );
            continue;
        };

        // Get current room name
        let current_name = room.name().unwrap_or_default();
        let expected_old_name = format!("{}: {}", old_prefix, channel.channel_name);
        let new_name = format!("{}: {}", new_prefix, channel.channel_name);

        // Only rename if it matches the old prefix pattern
        if current_name == expected_old_name {
            tracing::info!(
                channel = %channel.channel_name,
                old_name = %current_name,
                new_name = %new_name,
                "Renaming room"
            );

            let content = RoomNameEventContent::new(new_name.clone());
            match room.send_state_event(content).await {
                Ok(_) => {
                    tracing::info!(
                        channel = %channel.channel_name,
                        new_name = %new_name,
                        "Room renamed successfully"
                    );
                }
                Err(e) => {
                    tracing::error!(
                        channel = %channel.channel_name,
                        error = %e,
                        "Failed to rename room"
                    );
                }
            }
        } else {
            tracing::debug!(
                channel = %channel.channel_name,
                current_name = %current_name,
                expected = %expected_old_name,
                "Room name doesn't match old prefix pattern, skipping"
            );
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        None => {
            // No subcommand - launch GUI (desktop app mode)
            gorp::gui::run_gui()
        }
        Some(Commands::Start) => run_start().await,
        Some(Commands::Config { action }) => run_config(action),
        Some(Commands::Schedule { action }) => run_schedule(action),
        Some(Commands::Rooms { action }) => run_rooms(action).await,
    }
}

/// Handle config subcommands
fn run_config(action: ConfigAction) -> Result<()> {
    dotenvy::dotenv().ok();

    match action {
        ConfigAction::Init { force } => {
            let config_dir = paths::config_dir();
            let config_file = paths::config_file();

            // Check if config already exists
            if config_file.exists() && !force {
                eprintln!("Config file already exists: {}", config_file.display());
                eprintln!("Use --force to overwrite");
                std::process::exit(1);
            }

            // Create config directory
            std::fs::create_dir_all(&config_dir)?;

            // Write example config
            let example_config = include_str!("../config.toml.example");
            std::fs::write(&config_file, example_config)?;

            println!("✓ Created config directory: {}", config_dir.display());
            println!("✓ Created config file: {}", config_file.display());
            println!("\nEdit the config file to add your Matrix credentials.");
            println!("Then run: gorp config check");
            Ok(())
        }
        ConfigAction::Check => {
            print!("Checking configuration... ");
            match Config::load() {
                Ok(config) => {
                    println!("✓ Valid");
                    println!("\nConfiguration summary:");
                    println!("  Homeserver:    {}", config.matrix.home_server);
                    println!("  User ID:       {}", config.matrix.user_id);
                    println!("  Allowed users: {}", config.matrix.allowed_users.len());
                    println!("  Workspace:     {}", config.workspace.path);
                    println!("  Webhook port:  {}", config.webhook.port);
                    println!("  Timezone:      {}", config.scheduler.timezone);
                    Ok(())
                }
                Err(e) => {
                    println!("✗ Invalid");
                    eprintln!("\nError: {}", e);
                    std::process::exit(1);
                }
            }
        }
        ConfigAction::Show => {
            let config = Config::load()?;
            println!("[matrix]");
            println!("home_server = \"{}\"", config.matrix.home_server);
            println!("user_id = \"{}\"", config.matrix.user_id);
            println!("device_name = \"{}\"", config.matrix.device_name);
            println!(
                "password = \"{}\"",
                if config.matrix.password.is_some() {
                    "********"
                } else {
                    "<not set>"
                }
            );
            println!(
                "access_token = \"{}\"",
                if config.matrix.access_token.is_some() {
                    "********"
                } else {
                    "<not set>"
                }
            );
            println!(
                "recovery_key = \"{}\"",
                if config.matrix.recovery_key.is_some() {
                    "********"
                } else {
                    "<not set>"
                }
            );
            println!("allowed_users = {:?}", config.matrix.allowed_users);
            println!("\n[workspace]");
            println!("path = \"{}\"", config.workspace.path);
            println!("\n[backend]");
            println!("type = \"{}\"", config.backend.backend_type);
            if let Some(ref binary) = config.backend.binary {
                println!("binary = \"{}\"", binary);
            } else {
                println!("binary = \"claude-code-acp\" # Not configured, using default");
            }
            println!("\n[webhook]");
            println!("port = {}", config.webhook.port);
            println!("\n[scheduler]");
            println!("timezone = \"{}\"", config.scheduler.timezone);
            Ok(())
        }
        ConfigAction::Path => {
            println!("{}", paths::config_file().display());
            Ok(())
        }
    }
}

/// Handle schedule subcommands
fn run_schedule(action: ScheduleAction) -> Result<()> {
    dotenvy::dotenv().ok();
    let config = Config::load()?;
    let session_store = SessionStore::new(&config.workspace.path)?;
    let scheduler_store = SchedulerStore::new(session_store.db_connection());
    scheduler_store.initialize_schema()?;

    match action {
        ScheduleAction::List => {
            let schedules = scheduler_store.list_all()?;
            if schedules.is_empty() {
                println!("No scheduled tasks.");
            } else {
                println!(
                    "{:<8} {:<10} {:<20} Prompt",
                    "ID", "Status", "Next Execution"
                );
                println!("{}", "-".repeat(70));
                for s in schedules {
                    let next = if s.next_execution_at.is_empty() {
                        "-".to_string()
                    } else {
                        // Truncate to just date and time
                        s.next_execution_at.chars().take(16).collect()
                    };
                    let prompt_preview: String = s.prompt.chars().take(30).collect();
                    println!(
                        "{:<8} {:<10} {:<20} {}{}",
                        &s.id[..8],
                        format!("{:?}", s.status),
                        next,
                        prompt_preview,
                        if s.prompt.len() > 30 { "..." } else { "" }
                    );
                }
            }
            Ok(())
        }
        ScheduleAction::Clear { force } => {
            let schedules = scheduler_store.list_all()?;
            if schedules.is_empty() {
                println!("No scheduled tasks to clear.");
                return Ok(());
            }

            if !force {
                print!(
                    "This will delete {} scheduled task(s). Continue? [y/N] ",
                    schedules.len()
                );
                use std::io::Write;
                std::io::stdout().flush()?;

                let mut input = String::new();
                std::io::stdin().read_line(&mut input)?;
                if !input.trim().eq_ignore_ascii_case("y") {
                    println!("Cancelled.");
                    return Ok(());
                }
            }

            for s in &schedules {
                scheduler_store.delete_schedule(&s.id)?;
            }
            println!("Cleared {} scheduled task(s).", schedules.len());
            Ok(())
        }
    }
}

/// Handle rooms subcommands
async fn run_rooms(action: RoomsAction) -> Result<()> {
    dotenvy::dotenv().ok();
    let config = Config::load()?;
    let session_store = SessionStore::new(&config.workspace.path)?;

    match action {
        RoomsAction::Sync => {
            println!(
                "Syncing room names to prefix: {}",
                config.matrix.room_prefix
            );

            // Need to login to Matrix to rename rooms
            let client = matrix_client::create_client(
                &config.matrix.home_server,
                &config.matrix.user_id,
                &config.matrix.device_name,
            )
            .await?;

            matrix_client::login(
                &client,
                &config.matrix.user_id,
                config.matrix.password.as_deref(),
                config.matrix.access_token.as_deref(),
                &config.matrix.device_name,
            )
            .await?;

            // Do initial sync to get room list
            print!("Syncing with server... ");
            client
                .sync_once(SyncSettings::default())
                .await
                .context("Initial sync failed")?;
            println!("done.");

            // Get all channels and rename their rooms
            let channels = session_store.list_all()?;
            let prefix = &config.matrix.room_prefix;

            for channel in &channels {
                let room_id: OwnedRoomId = match channel.room_id.parse() {
                    Ok(id) => id,
                    Err(_) => {
                        println!("  ✗ {}: invalid room ID", channel.channel_name);
                        continue;
                    }
                };

                let Some(room) = client.get_room(&room_id) else {
                    println!("  ✗ {}: room not found", channel.channel_name);
                    continue;
                };

                let new_name = format!("{}: {}", prefix, channel.channel_name);
                let current_name = room.name().unwrap_or_default();

                if current_name == new_name {
                    println!("  ✓ {}: already correct", channel.channel_name);
                    continue;
                }

                let content = RoomNameEventContent::new(new_name.clone());
                match room.send_state_event(content).await {
                    Ok(_) => {
                        println!(
                            "  ✓ {}: \"{}\" → \"{}\"",
                            channel.channel_name, current_name, new_name
                        );
                    }
                    Err(e) => {
                        println!("  ✗ {}: {}", channel.channel_name, e);
                    }
                }
            }

            // Update stored prefix
            session_store.set_setting(SETTING_ROOM_PREFIX, prefix)?;
            println!("\nDone. Renamed {} room(s).", channels.len());
            Ok(())
        }
    }
}

/// Start the Matrix-Claude bridge
async fn run_start() -> Result<()> {
    // Set up panic hook to log panics before they crash the process
    std::panic::set_hook(Box::new(|panic_info| {
        eprintln!("\n╔══════════════════════════════════════════════════════════╗");
        eprintln!("║ PANIC! Bot crashed with the following error:            ║");
        eprintln!("╚══════════════════════════════════════════════════════════╝\n");
        eprintln!("{}", panic_info);
        eprintln!("\nBacktrace:");
        eprintln!("{:?}", std::backtrace::Backtrace::force_capture());
    }));

    // Initialize dual logging: JSON file (debug) + pretty console (warn+)
    let log_dir = paths::log_dir();
    std::fs::create_dir_all(&log_dir).expect("Failed to create log directory");

    // File appender for JSON logs (rotates daily)
    let file_appender = tracing_appender::rolling::daily(&log_dir, "debug.log");
    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);

    // JSON file layer - captures everything at debug level
    let file_layer = fmt::layer()
        .json()
        .with_writer(non_blocking)
        .with_span_events(FmtSpan::CLOSE)
        .with_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                "debug,matrix_sdk_crypto::backups=error,matrix_sdk_crypto::session_manager::sessions=error,matrix_sdk_crypto::machine=error,hyper=info,tower=info".into()
            }),
        );

    // Console layer - pretty output filtered to warn+
    // Suppress noisy SDK warnings (still logged to file)
    let console_layer =
        fmt::layer()
            .pretty()
            .with_target(true)
            .with_filter(tracing_subscriber::EnvFilter::new(
                "warn,gorp=info,matrix_sdk_crypto=error,matrix_sdk::encryption=error",
            ));

    tracing_subscriber::registry()
        .with(file_layer)
        .with(console_layer)
        .init();

    tracing::info!("Starting gorp - Matrix-Claude Bridge");

    // Log PATH for debugging ACP spawn issues
    if let Ok(path) = std::env::var("PATH") {
        tracing::info!(path_len = path.len(), "Environment PATH length");
        if path.contains("mise") {
            tracing::debug!("PATH contains mise directories");
        } else {
            tracing::warn!("PATH does not contain mise - node spawning may fail");
        }
    } else {
        tracing::error!("No PATH environment variable set!");
    }

    // Load configuration
    dotenvy::dotenv().ok();
    let config = Config::load()?;

    tracing::info!(
        homeserver = %config.matrix.home_server,
        user_id = %config.matrix.user_id,
        allowed_users = config.matrix.allowed_users.len(),
        workspace = %config.workspace.path,
        webhook_port = config.webhook.port,
        "Configuration loaded"
    );

    // Initialize server state (single source of truth - shared with GUI mode)
    let server = gorp::server::ServerState::initialize(config).await?;

    // Extract fields for use in headless-specific code
    let config_arc = Arc::clone(&server.config);
    let session_store_arc = Arc::clone(&server.session_store);
    let scheduler_store = server.scheduler_store.clone();
    let warm_manager = server.warm_manager.clone();
    let client = server.matrix_client.clone();
    let sync_token = server.sync_token.clone();

    // Start webhook server in background (can run before initial sync)
    let webhook_port = config_arc.webhook.port;
    let webhook_store = (*session_store_arc).clone();
    let webhook_client = client.clone();
    let webhook_config_arc = Arc::clone(&config_arc);
    let webhook_warm_manager = warm_manager.clone();
    tokio::spawn(async move {
        if let Err(e) = webhook::start_webhook_server(
            webhook_port,
            webhook_store,
            webhook_client,
            webhook_config_arc,
            webhook_warm_manager,
        )
        .await
        {
            tracing::error!(error = %e, "Webhook server failed");
        }
    });

    // Clone scheduler_store for message handler before moving into background task
    let scheduler_store_for_handler = scheduler_store.clone();

    // Start scheduler background task (checks every 60 seconds)
    // Note: Scheduler needs LocalSet because ACP client futures are !Send
    let scheduler_session_store = (*session_store_arc).clone();
    let scheduler_client = client.clone();
    let scheduler_config = Arc::clone(&config_arc);
    let scheduler_warm_manager = warm_manager.clone();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("Failed to create scheduler runtime");

        let local = tokio::task::LocalSet::new();
        local.block_on(&rt, async move {
            start_scheduler(
                scheduler_store,
                scheduler_session_store,
                scheduler_client,
                scheduler_config,
                Duration::from_secs(60),
                scheduler_warm_manager,
            )
            .await;
        });
    });

    // Initial sync was performed by ServerState::initialize()
    // Record startup time AFTER initial sync completes
    // This filters out historical messages from the initial sync batch
    let startup_time = chrono::Utc::now();
    STARTUP_TIME
        .set(startup_time)
        .expect("STARTUP_TIME already set");
    tracing::info!(startup_time = %startup_time, "Startup time recorded - will ignore messages before this");

    // Set up cross-signing for device verification
    // Only recover with valid recovery key - never auto-bootstrap (creates new keys silently)
    let cross_signing_ready = if let Some(recovery_key) = &config_arc.matrix.recovery_key {
        let cleaned_key = recovery_key.trim();

        if cleaned_key.is_empty() {
            tracing::warn!("Recovery key is empty - skipping cross-signing setup");
            false
        } else if !is_valid_recovery_key_format(cleaned_key) {
            tracing::error!("Recovery key format appears invalid");
            tracing::error!("Expected format: 'EsTR mwqJ JoXZ 8dKN ...' (4-letter groups)");
            tracing::error!(
                "Get the correct key from Element: Settings > Security > Secure Backup"
            );
            false
        } else {
            tracing::info!("Attempting to recover secrets using recovery key...");
            match client.encryption().recovery().recover(cleaned_key).await {
                Ok(()) => {
                    tracing::info!("Successfully recovered cross-signing secrets");

                    // Verify our own identity to complete self-signing
                    if let Some(user_id) = client.user_id() {
                        match client.encryption().get_user_identity(user_id).await {
                            Ok(Some(identity)) => {
                                if let Err(e) = identity.verify().await {
                                    tracing::warn!(error = %e, "Failed to verify own identity");
                                } else {
                                    tracing::info!("Own identity verified - device is now trusted");
                                }
                            }
                            Ok(None) => {
                                tracing::warn!("Own user identity not found");
                            }
                            Err(e) => {
                                tracing::warn!(error = %e, "Failed to get own identity");
                            }
                        }
                    }

                    // Check backup state
                    let backup_state = client.encryption().backups().state();
                    tracing::info!(state = ?backup_state, "Backup state after recovery");

                    true
                }
                Err(e) => {
                    tracing::error!(error = %e, "Recovery key was rejected by server");
                    tracing::error!("This usually means the key is incorrect or was reset");
                    tracing::error!(
                        "Get the correct key from Element: Settings > Security > Secure Backup"
                    );
                    false
                }
            }
        }
    } else {
        tracing::info!("No recovery key configured - device will be unverified");
        tracing::info!("To verify this device, either:");
        tracing::info!(
            "  1. Add recovery_key to config.toml (from Element > Security > Secure Backup)"
        );
        tracing::info!("  2. Or manually verify from Element's Security settings");
        false
    };

    if !cross_signing_ready {
        tracing::warn!("Device is UNVERIFIED - other users will see security warnings");
        tracing::warn!("Encrypted messaging will still work, but messages show as unverified");
    }

    // Check if room prefix changed and rename rooms if needed
    check_and_rename_rooms_for_prefix_change(&client, &config_arc, &session_store_arc).await;

    // Create a channel for message events - handlers will send events here
    // A LocalSet task will receive and process them, ensuring spawn_local works
    let (msg_tx, mut msg_rx) = tokio::sync::mpsc::channel::<(
        Room,
        matrix_sdk::ruma::events::room::message::OriginalSyncRoomMessageEvent,
        Client,
        Arc<Config>,
        Arc<SessionStore>,
        SchedulerStore,
        SharedWarmSessionManager,
    )>(256);

    // NOW register event handlers after encryption is established
    // This prevents handlers from firing before the client is ready
    register_event_handlers(
        &client,
        &config_arc,
        &session_store_arc,
        scheduler_store_for_handler,
        warm_manager.clone(),
        msg_tx, // Pass the sender to the handler
    );
    tracing::info!("Event handlers registered");

    tracing::info!("Bot ready - DM me to create Claude rooms!");

    // Announce startup to management room
    announce_startup_to_management(&client).await;

    // Notify allowed users that the bot is ready
    notify_ready(&client, &config_arc).await;

    // Notify DISPATCH channels with contextual status
    dispatch_startup_notification(&client, &session_store_arc).await;

    // Start task executor for dispatched work
    start_task_executor(
        client.clone(),
        (*session_store_arc).clone(),
        Arc::clone(&config_arc),
        warm_manager.clone(),
    );

    // Start continuous sync loop with the sync token from initial sync
    // Use LocalSet because message handlers with ACP client futures are !Send
    let settings = SyncSettings::default().token(sync_token);
    tracing::info!("Starting continuous sync loop with LocalSet");

    let local = tokio::task::LocalSet::new();
    local.run_until(async move {
        // Spawn the message handler task inside the LocalSet
        // This ensures spawn_local works correctly
        let mut handler_task = tokio::task::spawn_local(async move {
            tracing::info!("Message handler LocalSet task started");

            // Event deduplication: track recently seen event IDs to prevent
            // processing the same Matrix event multiple times (can happen during
            // sync reconnections or SDK event delivery quirks)
            let mut deduplicator = EventDeduplicator::new(10000);

            while let Some((room, event, client, config, session_store, scheduler, warm_mgr)) = msg_rx.recv().await {
                // Deduplicate by event_id - skip if we've already processed this event
                let event_id = event.event_id.to_string();
                if !deduplicator.check_and_mark(&event_id) {
                    tracing::debug!(
                        event_id = %event_id,
                        room_id = %room.room_id(),
                        "Skipping duplicate event - already processed"
                    );
                    continue;
                }

                let room_id = room.room_id().to_owned();
                tracing::info!(room_id = %room_id, event_id = %event_id, "Spawning concurrent message handler");
                // Spawn each message handler concurrently instead of awaiting sequentially
                tokio::task::spawn_local(async move {
                    tracing::info!(room_id = %room_id, "Processing message concurrently");
                    if let Err(e) = message_handler::handle_message(
                        room,
                        event,
                        client,
                        (*config).clone(),
                        (*session_store).clone(),
                        scheduler,
                        warm_mgr,
                    )
                    .await
                    {
                        tracing::error!(room_id = %room_id, error = %e, "Error handling message");
                    }
                });
            }
            tracing::warn!("Message handler channel closed");
        });

        // Yield to let the handler task start before sync
        tokio::task::yield_now().await;
        tracing::info!("Handler task spawned, starting sync");

        // Run sync - let the SDK handle reconnection internally
        // The SDK's sync() is designed to run forever with built-in retry logic.
        // Previously we wrapped this in a 90-second timeout, but that can cause
        // state corruption when cancelled mid-operation, leading to duplicate events.
        // If the handler task exits, we'll exit too.
        tokio::select! {
            sync_result = client.sync(settings.clone()) => {
                match sync_result {
                    Ok(_) => {
                        // Sync completed normally (shouldn't happen, sync is infinite)
                        tracing::warn!("Matrix sync returned unexpectedly");
                        Ok(())
                    }
                    Err(e) => {
                        // Sync error - this is fatal, let it propagate
                        tracing::error!(error = %e, "Matrix sync failed");
                        Err(e)
                    }
                }
            }
            _ = &mut handler_task => {
                tracing::error!("Message handler task exited unexpectedly");
                Err(matrix_sdk::Error::UnknownError(Box::new(std::io::Error::other(
                    "Message handler exited"
                ))))
            }
        }
    }).await?;

    Ok(())
}

/// Registers all event handlers for the Matrix client.
/// Type alias for the message event channel
type MessageEventSender = tokio::sync::mpsc::Sender<(
    Room,
    matrix_sdk::ruma::events::room::message::OriginalSyncRoomMessageEvent,
    Client,
    Arc<Config>,
    Arc<SessionStore>,
    SchedulerStore,
    SharedWarmSessionManager,
)>;

/// Called AFTER initial sync to ensure encryption is established before processing events.
fn register_event_handlers(
    client: &Client,
    config_arc: &Arc<Config>,
    session_store_arc: &Arc<SessionStore>,
    scheduler_store: SchedulerStore,
    warm_manager: SharedWarmSessionManager,
    msg_tx: MessageEventSender,
) {
    let config_for_invite = Arc::clone(config_arc);
    let config_for_messages = Arc::clone(config_arc);
    let session_store_for_messages = Arc::clone(session_store_arc);
    let warm_manager_for_messages = warm_manager.clone();

    // Auto-join room invites from allowed users
    client.add_event_handler(
        move |ev: matrix_sdk::ruma::events::room::member::StrippedRoomMemberEvent,
              client: Client,
              room: matrix_sdk::room::Room| {
            let config = Arc::clone(&config_for_invite);
            async move {
                if ev.state_key != client.user_id().unwrap() {
                    return; // Not an invite for us
                }

                if room.state() != matrix_sdk::RoomState::Invited {
                    return; // Not an invite
                }

                // Check if inviter is in allowed_users
                let allowed_users = config.allowed_users_set();
                let inviter = ev.sender.as_str();

                if allowed_users.contains(inviter) {
                    tracing::info!(
                        room_id = %room.room_id(),
                        inviter = %inviter,
                        "Auto-joining room invite from allowed user"
                    );

                    match room.join().await {
                        Ok(_) => {
                            tracing::info!(
                                room_id = %room.room_id(),
                                "Successfully joined room"
                            );
                        }
                        Err(e) => {
                            tracing::error!(
                                error = %e,
                                room_id = %room.room_id(),
                                "Failed to join room"
                            );
                        }
                    }
                } else {
                    tracing::warn!(
                        room_id = %room.room_id(),
                        inviter = %inviter,
                        "Ignoring room invite from unauthorized user"
                    );
                }
            }
        },
    );

    // Register message handler - send events through channel to LocalSet task
    // This ensures spawn_local in message_handler works correctly
    client.add_event_handler(move |event: SyncRoomMessageEvent, room: Room, client: Client| {
        let config = Arc::clone(&config_for_messages);
        let session_store = Arc::clone(&session_store_for_messages);
        let scheduler = scheduler_store.clone();
        let warm_mgr = warm_manager_for_messages.clone();
        let tx = msg_tx.clone();
        async move {
            // Extract and clone original message event before sending
            let Some(original_event) = event.as_original().cloned() else {
                return;
            };

            // Skip historical messages from before bot startup
            // This prevents processing old backlog when container restarts
            if let Some(startup_time) = STARTUP_TIME.get() {
                let msg_secs: i64 = original_event.origin_server_ts.as_secs().into();
                if is_message_before_startup(msg_secs, startup_time) {
                    tracing::debug!(
                        room_id = %room.room_id(),
                        msg_timestamp_secs = msg_secs,
                        startup_time = %startup_time,
                        "Skipping historical message from before startup"
                    );
                    return;
                }
            }

            // Send to LocalSet task for processing (ensures spawn_local context)
            tracing::debug!(room_id = %room.room_id(), "Sending message event to LocalSet handler");
            if let Err(e) = tx.send((room, original_event, client, config, session_store, scheduler, warm_mgr)).await {
                tracing::error!(error = %e, "Failed to send message to handler channel");
            }
        }
    });

    // Register verification event handler with proper error handling
    client.add_event_handler(
        |ev: matrix_sdk::ruma::events::key::verification::request::ToDeviceKeyVerificationRequestEvent,
         client: Client| async move {
            let Some(request) = client
                .encryption()
                .get_verification_request(&ev.sender, &ev.content.transaction_id)
                .await
            else {
                tracing::warn!(
                    sender = %ev.sender,
                    "Verification request not found"
                );
                return;
            };

            if let Err(e) = request.accept().await {
                tracing::error!(
                    error = %e,
                    sender = %ev.sender,
                    "Failed to accept verification request"
                );
            }
        },
    );

    // Register SAS verification handler (emoji verification)
    // WARNING: Auto-confirmation is a security risk in production environments.
    // For production, implement manual verification via admin interface.
    client.add_event_handler(
        |ev: matrix_sdk::ruma::events::key::verification::start::ToDeviceKeyVerificationStartEvent,
         client: Client| async move {
            let Some(verification) = client
                .encryption()
                .get_verification(&ev.sender, ev.content.transaction_id.as_str())
                .await
            else {
                tracing::warn!(
                    sender = %ev.sender,
                    "Verification not found for SAS start event"
                );
                return;
            };

            if let matrix_sdk::encryption::verification::Verification::SasV1(sas) = verification {
                tracing::info!(
                    sender = %ev.sender,
                    "Accepting SAS verification request"
                );

                if let Err(e) = sas.accept().await {
                    tracing::error!(
                        error = %e,
                        sender = %ev.sender,
                        "Failed to accept SAS verification"
                    );
                    return;
                }

                // Handle verification state changes in background task
                tokio::spawn(async move {
                    let mut stream = sas.changes();
                    while let Some(state) = stream.next().await {
                        use matrix_sdk::encryption::verification::SasState;

                        match state {
                            SasState::KeysExchanged {
                                emojis: Some(emoji_list),
                                ..
                            } => {
                                // Log emojis for manual verification if needed
                                tracing::warn!(
                                    "Emoji verification required - emojis displayed below"
                                );
                                for emoji in emoji_list.emojis.iter() {
                                    tracing::warn!(
                                        emoji = emoji.symbol,
                                        description = emoji.description,
                                        "Verification emoji"
                                    );
                                }
                                // WARNING: Auto-confirm is insecure - allows MITM attacks
                                // TODO: Implement proper verification for production
                                tracing::warn!(
                                    "Auto-confirming verification (INSECURE - for testing only)"
                                );
                                tokio::time::sleep(Duration::from_secs(5)).await;
                                if let Err(e) = sas.confirm().await {
                                    tracing::error!(
                                        error = %e,
                                        "Failed to confirm SAS verification"
                                    );
                                }
                            }
                            SasState::Done { .. } => {
                                let device = sas.other_device();
                                tracing::info!(
                                    user_id = %device.user_id(),
                                    device_id = %device.device_id(),
                                    "Successfully verified device"
                                );
                                break;
                            }
                            SasState::Cancelled(cancel_info) => {
                                tracing::warn!(
                                    reason = cancel_info.reason(),
                                    "Verification cancelled"
                                );
                                break;
                            }
                            _ => (),
                        }
                    }
                });
            }
        },
    );
}
