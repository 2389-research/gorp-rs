// ABOUTME: Matrix client initialization and authentication
// ABOUTME: Handles client creation with crypto store and login via password or token

use anyhow::{Context, Result};
use matrix_sdk::{
    ruma::OwnedUserId,
    Client,
};
use std::path::Path;

pub async fn create_client(homeserver: &str, _user_id: &str) -> Result<Client> {
    let client = Client::builder()
        .homeserver_url(homeserver)
        .sqlite_store(Path::new("./crypto_store"), None)
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
        let session = matrix_sdk::AuthSession::Matrix(matrix_sdk::matrix_auth::MatrixSession {
            meta: matrix_sdk::SessionMeta {
                user_id,
                device_id: device_name.to_string().into(),
            },
            tokens: matrix_sdk::matrix_auth::MatrixSessionTokens {
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

    tracing::info!(user_id = %client.user_id().unwrap(), "Logged in successfully");

    Ok(())
}
