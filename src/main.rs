// ABOUTME: Main entry point for Matrix-Claude bridge with sync loop
// ABOUTME: Initializes logging, config, session store, Matrix client, and message handlers

use anyhow::Result;
use matrix_bridge::{config::Config, matrix_client, message_handler, session::SessionStore};
use matrix_sdk::{config::SyncSettings, ruma::events::room::message::SyncRoomMessageEvent};
use std::sync::Arc;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    tracing::info!("Starting Matrix-Claude Bridge");

    // Load configuration
    dotenvy::dotenv().ok();
    let config = Config::from_env()?;

    tracing::info!(
        homeserver = %config.matrix_home_server,
        user_id = %config.matrix_user_id,
        room_id = %config.matrix_room_id,
        allowed_users = config.allowed_users.len(),
        "Configuration loaded"
    );

    // Initialize session store
    let session_store = SessionStore::new("./sessions_db")?;
    tracing::info!("Session store initialized");

    // Create Matrix client
    let client =
        matrix_client::create_client(&config.matrix_home_server, &config.matrix_user_id).await?;

    // Login
    matrix_client::login(
        &client,
        &config.matrix_user_id,
        config.matrix_password.as_deref(),
        config.matrix_access_token.as_deref(),
        &config.matrix_device_name,
    )
    .await?;

    // Join room
    let room_id: matrix_sdk::ruma::OwnedRoomId = config.matrix_room_id.parse()?;
    client.join_room_by_id(&room_id).await?;
    tracing::info!(room_id = %config.matrix_room_id, "Joined room");

    // Register message handler
    let config_clone = Arc::new(config);
    let session_store_clone = Arc::new(session_store);

    client.add_event_handler(move |event: SyncRoomMessageEvent, room, client| {
        let config = Arc::clone(&config_clone);
        let session_store = Arc::clone(&session_store_clone);
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

    tracing::info!("Message handler registered, starting sync loop");

    // Sync forever
    client.sync(SyncSettings::default()).await?;

    Ok(())
}
