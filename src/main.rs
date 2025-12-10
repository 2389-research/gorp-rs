// ABOUTME: Main entry point for Matrix-Claude bridge with sync loop
// ABOUTME: Initializes logging, config, session store, Matrix client, and message handlers

use anyhow::{Context, Result};
use futures_util::StreamExt;
use matrix_bridge::{
    config::Config, matrix_client, message_handler, session::SessionStore, webhook,
};
use matrix_sdk::{config::SyncSettings, ruma::events::room::message::SyncRoomMessageEvent, Client};
use std::sync::Arc;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

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

    tracing::info!("Bot ready - DM me to create Claude rooms!");

    // Register message handler
    let config_arc = Arc::new(config);
    let session_store_arc = Arc::new(session_store);

    // Clone Arcs for all handlers
    let webhook_config = Arc::clone(&config_arc);
    let webhook_session_store = Arc::clone(&session_store_arc);
    let config_for_invite = Arc::clone(&config_arc);
    let config_for_messages = Arc::clone(&config_arc);
    let session_store_for_messages = Arc::clone(&session_store_arc);

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

                    tracing::debug!("About to call room.join().await");
                    match room.join().await {
                        Ok(response) => {
                            tracing::info!(
                                room_id = %room.room_id(),
                                "Successfully joined room - join response received"
                            );
                            tracing::debug!("Join response: {:?}", response);
                        }
                        Err(e) => {
                            tracing::error!(
                                error = %e,
                                room_id = %room.room_id(),
                                "Failed to join room - error returned"
                            );
                        }
                    }
                    tracing::debug!("room.join().await completed without panic");
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

    // Register verification event handler
    client.add_event_handler(
        |ev: matrix_sdk::ruma::events::key::verification::request::ToDeviceKeyVerificationRequestEvent,
         client: Client| async move {
            let request = client
                .encryption()
                .get_verification_request(&ev.sender, &ev.content.transaction_id)
                .await
                .expect("Request object must exist");

            request
                .accept()
                .await
                .expect("Can't accept verification request");
        },
    );

    // Register SAS verification handler (emoji verification)
    client.add_event_handler(
        |ev: matrix_sdk::ruma::events::key::verification::start::ToDeviceKeyVerificationStartEvent,
         client: Client| async move {
            if let Some(verification) = client
                .encryption()
                .get_verification(&ev.sender, ev.content.transaction_id.as_str())
                .await
            {
                if let matrix_sdk::encryption::verification::Verification::SasV1(sas) = verification
                {
                    tracing::info!(
                        sender = %ev.sender,
                        "Accepting SAS verification request"
                    );

                    sas.accept().await.expect("Can't accept SAS verification");

                    // Auto-confirm emojis (for bot use - in production you'd want manual confirmation)
                    tokio::spawn(async move {
                        let mut stream = sas.changes();
                        while let Some(state) = stream.next().await {
                            use matrix_sdk::encryption::verification::SasState;

                            match state {
                                SasState::KeysExchanged { emojis, .. } => {
                                    if let Some(emoji_list) = emojis {
                                        tracing::warn!(
                                            "🔐 Emoji verification - Please verify these emojis match on the other device:"
                                        );
                                        for emoji in emoji_list.emojis.iter() {
                                            tracing::warn!(
                                                emoji = emoji.symbol,
                                                description = emoji.description,
                                                "Emoji"
                                            );
                                        }
                                        tracing::warn!("Auto-confirming in 5 seconds...");
                                        tokio::time::sleep(tokio::time::Duration::from_secs(5))
                                            .await;
                                        sas.confirm()
                                            .await
                                            .expect("Can't confirm SAS verification");
                                    }
                                }
                                SasState::Done { .. } => {
                                    let device = sas.other_device();
                                    tracing::info!(
                                        user_id = %device.user_id(),
                                        device_id = %device.device_id(),
                                        "✅ Successfully verified device"
                                    );
                                    break;
                                }
                                SasState::Cancelled(cancel_info) => {
                                    tracing::warn!(
                                        reason = cancel_info.reason(),
                                        "❌ Verification cancelled"
                                    );
                                    break;
                                }
                                _ => (),
                            }
                        }
                    });
                }
            }
        },
    );

    tracing::info!("Message and verification handlers registered");

    // Start webhook server in background
    let webhook_port = webhook_config.webhook.port;
    let webhook_store = (*webhook_session_store).clone();
    let webhook_client = client.clone();
    let webhook_config_arc = Arc::clone(&webhook_config);
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

    // Perform initial sync to upload device keys and establish encryption
    tracing::info!("Performing initial sync to set up encryption...");
    let response = client
        .sync_once(SyncSettings::default())
        .await
        .context("Initial sync failed")?;

    tracing::info!("Initial sync complete - encryption keys exchanged");

    // Start continuous sync loop with the sync token from initial sync
    let settings = SyncSettings::default().token(response.next_batch);
    tracing::info!("Starting continuous sync loop");
    client.sync(settings).await?;

    Ok(())
}
