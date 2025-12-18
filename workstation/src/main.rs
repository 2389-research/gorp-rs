// ABOUTME: Workstation webapp entry point - starts the Axum server.
// ABOUTME: Serves htmx UI for workspace configuration.

use anyhow::Result;
use tower_sessions_rusqlite_store::{tokio_rusqlite::Connection, RusqliteStore};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use workstation::{config::Config, gorp_client::GorpClient, oidc::OidcConfig, AppState};

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "workstation=debug,tower_http=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    tracing::info!("Starting workstation webapp");

    let config = Config::load()?;
    let oidc = OidcConfig::init(
        &config.oidc_issuer,
        &config.oidc_redirect_uri,
        &config.session_db_path,
    )
    .await?;
    let gorp = GorpClient::new(&config.gorp_api_url);

    // Initialize SQLite session store
    let session_conn = Connection::open(&config.session_db_path).await?;
    let session_store = RusqliteStore::new(session_conn);
    session_store.migrate().await?;
    tracing::info!("Session store initialized at {}", config.session_db_path);

    let state = AppState {
        config: config.clone(),
        oidc,
        gorp,
    };

    let app = workstation::routes::create_router(state, session_store);

    let addr = format!("0.0.0.0:{}", config.port);
    tracing::info!("Listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
