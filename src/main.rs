// ABOUTME: Main entry point for Matrix-Claude bridge with sync loop
// ABOUTME: Initializes logging, config, session store, Matrix client, and message handlers

use anyhow::{Context, Result};
use futures_util::StreamExt;
use matrix_bridge::{
    config::Config, matrix_client, message_handler, session::SessionStore, webhook,
};
use matrix_sdk::{config::SyncSettings, ruma::events::room::message::SyncRoomMessageEvent, Client};
use std::sync::Arc;
use std::time::Duration;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

/// Timeout for initial sync operation (uploading device keys, receiving room state)
const INITIAL_SYNC_TIMEOUT_SECS: u64 = 60;

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

    // Initialize logging
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                // Default to info, but suppress backup and crypto warnings
                "info,matrix_sdk_crypto::backups=error,matrix_sdk_crypto::session_manager::sessions=error".into()
            }),
        )
        .with(tracing_subscriber::fmt::layer())
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

    // Create Matrix client
    let client =
        matrix_client::create_client(&config.matrix.home_server, &config.matrix.user_id).await?;

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

    // NOW register event handlers after encryption is established
    // This prevents handlers from firing before the client is ready
    register_event_handlers(&client, &config_arc, &session_store_arc);
    tracing::info!("Event handlers registered");

    tracing::info!("Bot ready - DM me to create Claude rooms!");

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

    // Register message handler
    client.add_event_handler(move |event: SyncRoomMessageEvent, room, client| {
        let config = Arc::clone(&config_for_messages);
        let session_store = Arc::clone(&session_store_for_messages);
        async move {
            // Extract original message event
            let Some(original_event) = event.as_original() else {
                return;
            };

            if let Err(e) = message_handler::handle_message(
                room,
                original_event.clone(),
                client,
                (*config).clone(),
                (*session_store).clone(),
            )
            .await
            {
                tracing::error!(error = %e, "Error handling message");
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
