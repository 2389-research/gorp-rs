// ABOUTME: Matrix client initialization and authentication
// ABOUTME: Handles client creation with crypto store and login via password or token

use crate::paths;
use anyhow::{Context, Result};
use matrix_sdk::{
    authentication::{matrix::MatrixSession, SessionTokens},
    ruma::{
        api::client::room::create_room::v3::Request as CreateRoomRequest,
        assign,
        events::{room::encryption::RoomEncryptionEventContent, InitialStateEvent},
        OwnedRoomId, OwnedUserId,
    },
    AuthSession, Client, SessionMeta,
};

/// Convert a string to a filesystem-safe slug
fn slugify(s: &str) -> String {
    s.trim_start_matches('@')
        .replace(':', "_")
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '_' || *c == '.' || *c == '-')
        .collect()
}

pub async fn create_client(homeserver: &str, user_id: &str, device_name: &str) -> Result<Client> {
    // Include both user and device in path for full isolation
    let user_slug = slugify(user_id);
    let device_slug = slugify(device_name);
    let crypto_store_path =
        paths::crypto_store_dir().join(format!("{}_{}", user_slug, device_slug));

    // Ensure the crypto store directory exists
    std::fs::create_dir_all(&crypto_store_path)
        .context("Failed to create crypto store directory")?;

    tracing::info!(
        path = %crypto_store_path.display(),
        user = %user_slug,
        device = %device_slug,
        "Using crypto store directory"
    );

    let client = Client::builder()
        .homeserver_url(homeserver)
        .sqlite_store(&crypto_store_path, None)
        .build()
        .await
        .context("Failed to create Matrix client")?;

    tracing::info!("Matrix client created successfully");

    Ok(client)
}

pub async fn login(
    client: &Client,
    user_id: &str,
    password: Option<&str>,
    access_token: Option<&str>,
    device_name: &str,
) -> Result<()> {
    if let Some(token) = access_token {
        tracing::info!("Logging in with access token");
        let user_id: OwnedUserId = user_id.parse()?;
        let session = AuthSession::Matrix(MatrixSession {
            meta: SessionMeta {
                user_id,
                device_id: device_name.to_string().into(),
            },
            tokens: SessionTokens {
                access_token: token.to_string(),
                refresh_token: None,
            },
        });
        client.restore_session(session).await?;
    } else if let Some(pwd) = password {
        tracing::info!("Logging in with password");
        client
            .matrix_auth()
            .login_username(user_id, pwd)
            .device_id(device_name)
            .send()
            .await
            .context("Failed to log in")?;
    } else {
        anyhow::bail!("Either MATRIX_PASSWORD or MATRIX_ACCESS_TOKEN is required");
    }

    if let Some(user_id) = client.user_id() {
        tracing::info!(user_id = %user_id, "Logged in successfully");
    } else {
        tracing::warn!("Login succeeded but user_id not available");
    }

    Ok(())
}

/// Create a new private encrypted room and return its ID
pub async fn create_room(client: &Client, room_name: &str) -> Result<OwnedRoomId> {
    tracing::info!(room_name, "Creating new private encrypted room");

    // Enable E2E encryption by default (uses MegolmV1AesSha2)
    let encryption_event = InitialStateEvent::with_empty_state_key(
        RoomEncryptionEventContent::with_recommended_defaults(),
    );

    let request = assign!(CreateRoomRequest::new(), {
        name: Some(room_name.to_string()),
        is_direct: true,
        visibility: matrix_sdk::ruma::api::client::room::Visibility::Private,
        preset: Some(matrix_sdk::ruma::api::client::room::create_room::v3::RoomPreset::TrustedPrivateChat),
        initial_state: vec![encryption_event.to_raw_any()],
    });

    let room = client
        .create_room(request)
        .await
        .context("Failed to create room")?;

    let room_id = room.room_id().to_owned();
    tracing::info!(%room_id, "Encrypted room created successfully");

    Ok(room_id)
}

/// Invite a user to a room
pub async fn invite_user(client: &Client, room_id: &OwnedRoomId, user_id: &str) -> Result<()> {
    tracing::info!(%room_id, user_id, "Inviting user to room");

    let user_id_parsed: OwnedUserId = user_id.parse()?;
    let room = client.get_room(room_id).context("Room not found")?;

    room.invite_user_by_id(&user_id_parsed)
        .await
        .context("Failed to invite user")?;

    tracing::info!(%room_id, user_id, "User invited successfully");

    Ok(())
}

/// Request verification with a user
pub async fn request_verification(_client: &Client, user_id: &str) -> Result<()> {
    tracing::info!(
        user_id,
        "Requesting verification with user (automatic after sync)"
    );

    // In matrix-sdk 0.7, verification is typically initiated by the other user
    // or happens automatically when they message us. We'll log that we're ready
    // for verification but won't force it.

    tracing::info!(
        user_id,
        "Bot is ready to accept verification requests from this user"
    );

    Ok(())
}

/// Create a direct message room with a user
pub async fn create_dm_room(client: &Client, user_id: &OwnedUserId) -> Result<OwnedRoomId> {
    tracing::info!(user_id = %user_id, "Creating DM room");

    // Enable E2E encryption by default
    let encryption_event = InitialStateEvent::with_empty_state_key(
        RoomEncryptionEventContent::with_recommended_defaults(),
    );

    let request = assign!(CreateRoomRequest::new(), {
        is_direct: true,
        visibility: matrix_sdk::ruma::api::client::room::Visibility::Private,
        preset: Some(matrix_sdk::ruma::api::client::room::create_room::v3::RoomPreset::TrustedPrivateChat),
        initial_state: vec![encryption_event.to_raw_any()],
        invite: vec![user_id.clone()],
    });

    let room = client
        .create_room(request)
        .await
        .context("Failed to create DM room")?;

    let room_id = room.room_id().to_owned();

    // Mark as direct message room
    if let Err(e) = room.set_is_direct(true).await {
        tracing::warn!(error = %e, "Failed to mark room as direct");
    }

    tracing::info!(%room_id, user_id = %user_id, "DM room created");

    Ok(room_id)
}
