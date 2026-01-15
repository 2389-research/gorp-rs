// ABOUTME: Server state shared between GUI and headless modes
// ABOUTME: Contains Matrix client, session store, scheduler, and warm session manager

use crate::config::Config;
use crate::scheduler::SchedulerStore;
use crate::session::SessionStore;
use crate::warm_session::SharedWarmSessionManager;
use anyhow::Result;
use futures_util::FutureExt;
use matrix_sdk::Client;
use std::sync::Arc;

/// Shared server state between GUI and background tasks.
/// The GUI is a view layer over this state - it doesn't reinvent the server.
pub struct ServerState {
    pub config: Arc<Config>,
    pub matrix_client: Client,
    pub session_store: Arc<SessionStore>,
    pub scheduler_store: SchedulerStore,
    pub warm_manager: SharedWarmSessionManager,
    /// Sync token from initial sync - used by headless mode to continue syncing
    pub sync_token: String,
}

impl std::fmt::Debug for ServerState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ServerState")
            .field("config", &"...")
            .field("matrix_client", &"<Client>")
            .field("session_store", &"<SessionStore>")
            .field("scheduler_store", &"<SchedulerStore>")
            .field("warm_manager", &"<WarmSessionManager>")
            .field("sync_token", &"<token>")
            .finish()
    }
}

/// Room information for GUI display
#[derive(Debug, Clone)]
pub struct RoomInfo {
    pub id: String,
    pub name: String,
    pub is_direct: bool,
    pub unread_count: u64,
}

impl ServerState {
    /// Get list of joined rooms for display
    pub fn get_rooms(&self) -> Vec<RoomInfo> {
        self.matrix_client
            .joined_rooms()
            .iter()
            .map(|room| {
                let name = room
                    .cached_display_name()
                    .map(|n| n.to_string())
                    .unwrap_or_else(|| room.room_id().to_string());

                RoomInfo {
                    id: room.room_id().to_string(),
                    name,
                    is_direct: room
                        .is_direct()
                        .now_or_never()
                        .and_then(|r| r.ok())
                        .unwrap_or(false),
                    unread_count: room.unread_notification_counts().notification_count,
                }
            })
            .collect()
    }

    /// Initialize all server components.
    /// This is the same initialization that `run_start()` does, extracted for reuse.
    pub async fn initialize(config: Config) -> Result<Self> {
        use crate::matrix_client;
        use crate::warm_session::{create_shared_manager, WarmConfig};
        use anyhow::Context;
        use matrix_sdk::config::SyncSettings;
        use std::time::Duration;

        // Create warm session manager
        let warm_config = WarmConfig {
            keep_alive_duration: Duration::from_secs(config.backend.keep_alive_secs),
            pre_warm_lead_time: Duration::from_secs(config.backend.pre_warm_secs),
            agent_binary: config
                .backend
                .binary
                .clone()
                .unwrap_or_else(|| "claude".to_string()),
            backend_type: config.backend.backend_type.clone(),
            model: config.backend.model.clone(),
            max_tokens: config.backend.max_tokens,
            global_system_prompt_path: config.backend.global_system_prompt_path.clone(),
            mcp_servers: config.backend.mcp_servers.clone(),
        };
        let warm_manager = create_shared_manager(warm_config);

        // Spawn cleanup task
        let cleanup_manager = warm_manager.clone();
        let cleanup_interval = config.backend.keep_alive_secs / 4;
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(cleanup_interval));
            loop {
                interval.tick().await;
                let mut manager = cleanup_manager.write().await;
                manager.cleanup_stale();
            }
        });

        // Initialize session store
        let session_store = SessionStore::new(&config.workspace.path)?;
        tracing::info!(workspace = %config.workspace.path, "Session store initialized");

        // Initialize scheduler store
        let scheduler_store = SchedulerStore::new(session_store.db_connection());
        scheduler_store.initialize_schema()?;
        tracing::info!("Scheduler store initialized");

        // Create Matrix client
        let client = matrix_client::create_client(
            &config.matrix.home_server,
            &config.matrix.user_id,
            &config.matrix.device_name,
        )
        .await?;

        // Login
        matrix_client::login(
            &client,
            &config.matrix.user_id,
            config.matrix.password.as_deref(),
            config.matrix.access_token.as_deref(),
            &config.matrix.device_name,
        )
        .await?;

        // Initial sync to establish encryption
        tracing::info!("Performing initial sync...");
        let sync_response = tokio::time::timeout(
            Duration::from_secs(60),
            client.sync_once(SyncSettings::default()),
        )
        .await
        .context("Initial sync timed out")?
        .context("Initial sync failed")?;
        tracing::info!("Initial sync complete");

        Ok(Self {
            config: Arc::new(config),
            matrix_client: client,
            session_store: Arc::new(session_store),
            scheduler_store,
            warm_manager,
            sync_token: sync_response.next_batch,
        })
    }
}
