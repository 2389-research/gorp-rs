// ABOUTME: Main entry point for Matrix-Claude bridge with sync loop
// ABOUTME: Initializes logging, config, session store, Matrix client, and message handlers

use anyhow::{Context, Result};
use futures_util::StreamExt;
use matrix_bridge::{
    config::Config, matrix_client, message_handler, paths,
    scheduler::{SchedulerStore, start_scheduler},
    session::SessionStore, webhook,
};
use matrix_sdk::{
    config::SyncSettings,
    ruma::{
        events::room::message::{RoomMessageEventContent, SyncRoomMessageEvent},
        OwnedUserId,
    },
    Client,
};
use std::sync::Arc;
use std::time::Duration;
use tracing_subscriber::{
    fmt::{self, format::FmtSpan},
    layer::SubscriberExt,
    util::SubscriberInitExt,
    Layer,
};

/// Timeout for initial sync operation (uploading device keys, receiving room state)
const INITIAL_SYNC_TIMEOUT_SECS: u64 = 60;

/// Validate recovery key format (Base58 encoded, typically 48+ chars with spaces)
fn is_valid_recovery_key_format(key: &str) -> bool {
    let cleaned: String = key.chars().filter(|c| !c.is_whitespace()).collect();

    // Recovery keys are typically 48-60 Base58 characters
    if cleaned.len() < 40 || cleaned.len() > 70 {
        return false;
    }

    // Base58 alphabet excludes 0, O, I, l to avoid ambiguity
    cleaned.chars().all(|c| {
        c.is_ascii_alphanumeric() && c != '0' && c != 'O' && c != 'I' && c != 'l'
    })
}

/// Notify allowed users that the bot is ready
async fn notify_ready(client: &Client, config: &Config) {
    let ready_messages = [
        "🌅 *stretches digital limbs* I have awakened. The bridge between worlds is open.",
        "⚡ Systems nominal. Encryption verified. Ready to serve.",
        "🎭 From the depths of silicon dreams, I rise. How may I assist?",
        "🌊 Like a message in a bottle finding shore, I've arrived. Ready when you are.",
        "🔮 The oracle is online. Ask, and you shall receive (code reviews).",
    ];

    // Pick a message based on current time for variety
    let idx = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as usize % ready_messages.len())
        .unwrap_or(0);
    let message = ready_messages[idx];

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
                .any(|target| target.to_string() == user_id.to_string());

            if is_direct && has_target {
                dm_room = Some(room);
                break;
            }
        }

        if let Some(room) = dm_room {
            match room.send(RoomMessageEventContent::text_plain(message)).await {
                Ok(_) => {
                    tracing::info!(user = %user_id, "Sent ready notification");
                }
                Err(e) => {
                    tracing::warn!(user = %user_id, error = %e, "Failed to send ready notification");
                }
            }
        } else {
            tracing::debug!(user = %user_id, "No existing DM room, skipping notification");
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
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
    let console_layer = fmt::layer()
        .pretty()
        .with_target(true)
        .with_filter(
            tracing_subscriber::EnvFilter::new(
                "warn,matrix_bridge=info,matrix_sdk_crypto=error,matrix_sdk::encryption=error",
            ),
        );

    tracing_subscriber::registry()
        .with(file_layer)
        .with(console_layer)
        .init();

    tracing::info!("Starting Matrix-Claude Bridge");

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

    // Initialize session store
    let session_store = SessionStore::new(&config.workspace.path)?;
    tracing::info!(workspace = %config.workspace.path, "Session store initialized");

    // Initialize scheduler store (shares database with session store)
    let scheduler_store = SchedulerStore::new(session_store.db_connection());
    scheduler_store.initialize_schema()?;
    tracing::info!("Scheduler store initialized");

    // Create Matrix client
    let client =
        matrix_client::create_client(&config.matrix.home_server, &config.matrix.user_id, &config.matrix.device_name).await?;

    // Login
    matrix_client::login(
        &client,
        &config.matrix.user_id,
        config.matrix.password.as_deref(),
        config.matrix.access_token.as_deref(),
        &config.matrix.device_name,
    )
    .await?;

    // Wrap config and session store in Arc for sharing across handlers
    let config_arc = Arc::new(config);
    let session_store_arc = Arc::new(session_store);

    // Start webhook server in background (can run before initial sync)
    let webhook_port = config_arc.webhook.port;
    let webhook_store = (*session_store_arc).clone();
    let webhook_client = client.clone();
    let webhook_config_arc = Arc::clone(&config_arc);
    tokio::spawn(async move {
        if let Err(e) = webhook::start_webhook_server(
            webhook_port,
            webhook_store,
            webhook_client,
            webhook_config_arc,
        )
        .await
        {
            tracing::error!(error = %e, "Webhook server failed");
        }
    });

    // Clone scheduler_store for message handler before moving into background task
    let scheduler_store_for_handler = scheduler_store.clone();

    // Start scheduler background task (checks every 60 seconds)
    let scheduler_session_store = (*session_store_arc).clone();
    let scheduler_client = client.clone();
    let scheduler_config = Arc::clone(&config_arc);
    tokio::spawn(async move {
        start_scheduler(
            scheduler_store,
            scheduler_session_store,
            scheduler_client,
            scheduler_config,
            Duration::from_secs(60),
        )
        .await;
    });

    // Perform initial sync BEFORE registering event handlers
    // This ensures device encryption keys are uploaded and room keys are received
    // before any events are processed. Prevents race conditions with encrypted rooms.
    tracing::info!("Performing initial sync to set up encryption...");
    let response = tokio::time::timeout(
        Duration::from_secs(INITIAL_SYNC_TIMEOUT_SECS),
        client.sync_once(SyncSettings::default()),
    )
    .await
    .context("Initial sync timed out - homeserver may be unresponsive")?
    .context("Initial sync failed - unable to establish encryption keys with homeserver")?;

    tracing::info!("Initial sync complete - encryption keys exchanged");

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
            tracing::error!("Get the correct key from Element: Settings > Security > Secure Backup");
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
                    tracing::error!("Get the correct key from Element: Settings > Security > Secure Backup");
                    false
                }
            }
        }
    } else {
        tracing::info!("No recovery key configured - device will be unverified");
        tracing::info!("To verify this device, either:");
        tracing::info!("  1. Add recovery_key to config.toml (from Element > Security > Secure Backup)");
        tracing::info!("  2. Or manually verify from Element's Security settings");
        false
    };

    if !cross_signing_ready {
        tracing::warn!("Device is UNVERIFIED - other users will see security warnings");
        tracing::warn!("Encrypted messaging will still work, but messages show as unverified");
    }

    // NOW register event handlers after encryption is established
    // This prevents handlers from firing before the client is ready
    register_event_handlers(&client, &config_arc, &session_store_arc, scheduler_store_for_handler);
    tracing::info!("Event handlers registered");

    tracing::info!("Bot ready - DM me to create Claude rooms!");

    // Notify allowed users that the bot is ready
    notify_ready(&client, &config_arc).await;

    // Start continuous sync loop with the sync token from initial sync
    let settings = SyncSettings::default().token(response.next_batch);
    tracing::info!("Starting continuous sync loop");
    client.sync(settings).await?;

    Ok(())
}

/// Registers all event handlers for the Matrix client.
/// Called AFTER initial sync to ensure encryption is established before processing events.
fn register_event_handlers(
    client: &Client,
    config_arc: &Arc<Config>,
    session_store_arc: &Arc<SessionStore>,
    scheduler_store: SchedulerStore,
) {
    let config_for_invite = Arc::clone(config_arc);
    let config_for_messages = Arc::clone(config_arc);
    let session_store_for_messages = Arc::clone(session_store_arc);

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

    // Register message handler - spawn each handler in background to avoid blocking
    client.add_event_handler(move |event: SyncRoomMessageEvent, room, client| {
        let config = Arc::clone(&config_for_messages);
        let session_store = Arc::clone(&session_store_for_messages);
        let scheduler = scheduler_store.clone();
        async move {
            // Extract and clone original message event before spawning
            let Some(original_event) = event.as_original().cloned() else {
                return;
            };

            // Spawn handler in background so Claude requests don't block other messages
            tokio::spawn(async move {
                if let Err(e) = message_handler::handle_message(
                    room,
                    original_event,
                    client,
                    (*config).clone(),
                    (*session_store).clone(),
                    scheduler,
                )
                .await
                {
                    tracing::error!(error = %e, "Error handling message");
                }
            });
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
                            SasState::KeysExchanged { emojis, .. } => {
                                if let Some(emoji_list) = emojis {
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
