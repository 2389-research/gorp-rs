// ABOUTME: Browser CDP management for Chrome DevTools Protocol integration.
// ABOUTME: Handles screencast streaming and remote control of browser instances.

use anyhow::{Context, Result};
use chromiumoxide::cdp::browser_protocol::input::{
    DispatchMouseEventParams, DispatchMouseEventType, MouseButton,
};
use chromiumoxide::cdp::browser_protocol::input::{
    InsertTextParams,
};
use chromiumoxide::cdp::browser_protocol::page::{
    CaptureScreenshotFormat, CaptureScreenshotParams,
};
use chromiumoxide::{Browser, BrowserConfig, Page};
use futures_util::StreamExt;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use uuid::Uuid;

/// Browser session for CDP streaming
pub struct BrowserSession {
    pub id: String,
    pub page: Page,
    shutdown_tx: Option<mpsc::Sender<()>>,
}

impl BrowserSession {
    /// Take a screenshot and return base64-encoded PNG
    pub async fn screenshot(&self) -> Result<String> {
        let params = CaptureScreenshotParams::builder()
            .format(CaptureScreenshotFormat::Png)
            .build();

        let screenshot = self.page.execute(params).await?;
        let data_string: String = screenshot.data.clone().into();
        Ok(data_string)
    }

    /// Navigate to a URL
    pub async fn navigate(&self, url: &str) -> Result<()> {
        self.page.goto(url).await?;
        Ok(())
    }

    /// Click at coordinates
    pub async fn click(&self, x: f64, y: f64) -> Result<()> {
        let pressed_params = DispatchMouseEventParams::builder()
            .r#type(DispatchMouseEventType::MousePressed)
            .x(x)
            .y(y)
            .button(MouseButton::Left)
            .click_count(1)
            .build()
            .map_err(|e| anyhow::anyhow!("Failed to build mouse pressed params: {}", e))?;

        self.page.execute(pressed_params).await?;

        let released_params = DispatchMouseEventParams::builder()
            .r#type(DispatchMouseEventType::MouseReleased)
            .x(x)
            .y(y)
            .button(MouseButton::Left)
            .click_count(1)
            .build()
            .map_err(|e| anyhow::anyhow!("Failed to build mouse released params: {}", e))?;

        self.page.execute(released_params).await?;
        Ok(())
    }

    /// Type text
    pub async fn type_text(&self, text: &str) -> Result<()> {
        let params = InsertTextParams::builder()
            .text(text)
            .build()
            .map_err(|e| anyhow::anyhow!("Failed to build insert text params: {}", e))?;

        self.page.execute(params).await?;
        Ok(())
    }

    /// Signal shutdown
    pub async fn shutdown(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(()).await;
        }
    }
}

/// Manages browser sessions
pub struct BrowserManager {
    browser: RwLock<Option<Browser>>,
    sessions: RwLock<HashMap<String, Arc<BrowserSession>>>,
}

impl BrowserManager {
    pub fn new() -> Self {
        Self {
            browser: RwLock::new(None),
            sessions: RwLock::new(HashMap::new()),
        }
    }

    /// Initialize browser if not already running
    async fn ensure_browser(&self) -> Result<()> {
        let mut browser_lock = self.browser.write().await;
        if browser_lock.is_none() {
            let (browser, mut handler) = Browser::launch(
                BrowserConfig::builder()
                    .window_size(1280, 720)
                    .build()
                    .map_err(|e| anyhow::anyhow!("Browser config error: {}", e))?,
            )
            .await
            .context("Failed to launch browser")?;

            // Spawn handler task
            tokio::spawn(async move {
                while let Some(event) = handler.next().await {
                    tracing::trace!(?event, "Browser event");
                }
            });

            *browser_lock = Some(browser);
            tracing::info!("Browser launched");
        }
        Ok(())
    }

    /// Create a new browser session
    pub async fn create_session(&self) -> Result<Arc<BrowserSession>> {
        self.ensure_browser().await?;

        let browser_lock = self.browser.read().await;
        let browser = browser_lock
            .as_ref()
            .context("Browser not initialized")?;

        let page = browser.new_page("about:blank").await?;
        let session_id = Uuid::new_v4().to_string();

        let (shutdown_tx, _shutdown_rx) = mpsc::channel::<()>(1);

        let session = Arc::new(BrowserSession {
            id: session_id.clone(),
            page,
            shutdown_tx: Some(shutdown_tx),
        });

        self.sessions
            .write()
            .await
            .insert(session_id, session.clone());
        Ok(session)
    }

    /// Get an existing session
    pub async fn get(&self, session_id: &str) -> Option<Arc<BrowserSession>> {
        self.sessions.read().await.get(session_id).cloned()
    }

    /// Remove a session
    pub async fn remove(&self, session_id: &str) -> Option<Arc<BrowserSession>> {
        self.sessions.write().await.remove(session_id)
    }
}

impl Default for BrowserManager {
    fn default() -> Self {
        Self::new()
    }
}
