// ABOUTME: Axum router setup for workstation webapp.
// ABOUTME: Defines all HTTP routes and middleware.

use askama::Template;
use axum::{
    extract::{Path, State, WebSocketUpgrade},
    http::StatusCode,
    response::{Html, IntoResponse},
    routing::{any, get, post},
    Router,
};
use tower_sessions::{Session, SessionManagerLayer};
use tower_sessions_rusqlite_store::RusqliteStore;

use crate::{
    auth::{self, get_current_user},
    files,
    templates::{BrowserTemplate, IndexTemplate, TerminalTemplate},
    AppState,
};

pub fn create_router(state: AppState, session_store: RusqliteStore) -> Router {
    let session_layer = SessionManagerLayer::new(session_store)
        .with_secure(false)
        .with_same_site(tower_sessions::cookie::SameSite::Lax);

    Router::new()
        .route("/", get(index))
        .route("/health", get(health))
        .route("/terminal", get(terminal))
        .route("/browser", get(browser))
        .route("/auth/login", get(auth::login))
        .route("/auth/callback", get(auth::callback))
        .route("/auth/logout", get(auth::logout))
        .route("/files/{channel}/{*path}", get(files::list_files))
        .route(
            "/edit/{channel}/{*path}",
            get(files::read_file).post(files::save_file),
        )
        // Gorp API proxy routes
        .route("/gorp/api/terminal", post(proxy_terminal_create))
        .route("/gorp/api/browser", post(proxy_browser_create))
        .route("/gorp/ws/terminal/{session_id}", any(proxy_ws_terminal))
        .route("/gorp/ws/browser/{session_id}", any(proxy_ws_browser))
        .layer(session_layer)
        .with_state(state)
}

async fn index(State(state): State<AppState>, session: Session) -> impl IntoResponse {
    let user = get_current_user(&session).await;

    let (channels, error) = if user.is_some() {
        match state.gorp.list_channels().await {
            Ok(c) => (c, None),
            Err(e) => (vec![], Some(e.to_string())),
        }
    } else {
        (vec![], None)
    };

    let template = IndexTemplate {
        user,
        channels,
        error,
    };
    Html(template.render().unwrap())
}

async fn health() -> &'static str {
    "ok"
}

async fn terminal(State(state): State<AppState>, session: Session) -> impl IntoResponse {
    let user = get_current_user(&session).await;

    // Convert http URL to ws URL
    let gorp_ws_url = state
        .config
        .gorp_api_url
        .replace("http://", "ws://")
        .replace("https://", "wss://");

    let template = TerminalTemplate {
        user,
        gorp_api_url: state.config.gorp_api_url.clone(),
        gorp_ws_url,
        workspace_path: state.config.workspace_path.clone(),
    };
    Html(template.render().unwrap())
}

async fn browser(State(_state): State<AppState>, session: Session) -> impl IntoResponse {
    let user = get_current_user(&session).await;

    // Use proxy URLs (relative to workstation)
    let template = BrowserTemplate {
        user,
        gorp_api_url: String::new(), // Will use relative /gorp/api/browser
        gorp_ws_url: String::new(),  // Will use relative /gorp/ws/browser
    };
    Html(template.render().unwrap())
}

// Gorp API proxy handlers
async fn proxy_terminal_create(State(state): State<AppState>) -> impl IntoResponse {
    let client = reqwest::Client::new();
    let url = format!("{}/admin/api/terminal", state.config.gorp_api_url);

    match client.post(&url)
        .json(&serde_json::json!({"workspace_path": state.config.workspace_path}))
        .send()
        .await
    {
        Ok(resp) => {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            // Rewrite the ws_url in response to use our proxy
            let body = body.replace("/admin/ws/terminal/", "/gorp/ws/terminal/");
            (StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::OK), body).into_response()
        }
        Err(e) => (StatusCode::BAD_GATEWAY, format!("Proxy error: {}", e)).into_response()
    }
}

async fn proxy_browser_create(State(state): State<AppState>) -> impl IntoResponse {
    let client = reqwest::Client::new();
    let url = format!("{}/admin/api/browser", state.config.gorp_api_url);

    match client.post(&url).send().await {
        Ok(resp) => {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            // Rewrite the ws_url in response to use our proxy
            let body = body.replace("/admin/ws/browser/", "/gorp/ws/browser/");
            (StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::OK), body).into_response()
        }
        Err(e) => (StatusCode::BAD_GATEWAY, format!("Proxy error: {}", e)).into_response()
    }
}

async fn proxy_ws_terminal(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    let gorp_ws_url = format!(
        "{}/admin/ws/terminal/{}",
        state.config.gorp_api_url.replace("http://", "ws://").replace("https://", "wss://"),
        session_id
    );

    ws.on_upgrade(move |socket| proxy_websocket(socket, gorp_ws_url))
}

async fn proxy_ws_browser(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    let gorp_ws_url = format!(
        "{}/admin/ws/browser/{}",
        state.config.gorp_api_url.replace("http://", "ws://").replace("https://", "wss://"),
        session_id
    );

    ws.on_upgrade(move |socket| proxy_websocket(socket, gorp_ws_url))
}

async fn proxy_websocket(client_socket: axum::extract::ws::WebSocket, gorp_url: String) {
    use axum::extract::ws::Message;
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::connect_async;

    // Connect to gorp WebSocket
    let (gorp_socket, _) = match connect_async(&gorp_url).await {
        Ok(s) => s,
        Err(e) => {
            tracing::error!(error = %e, url = %gorp_url, "Failed to connect to gorp WebSocket");
            return;
        }
    };

    let (mut gorp_sink, mut gorp_stream) = gorp_socket.split();
    let (mut client_sink, mut client_stream) = client_socket.split();

    // Forward client -> gorp
    let client_to_gorp = async {
        while let Some(msg) = client_stream.next().await {
            match msg {
                Ok(Message::Text(text)) => {
                    if gorp_sink.send(tokio_tungstenite::tungstenite::Message::Text(text.to_string())).await.is_err() {
                        break;
                    }
                }
                Ok(Message::Binary(data)) => {
                    if gorp_sink.send(tokio_tungstenite::tungstenite::Message::Binary(data.to_vec())).await.is_err() {
                        break;
                    }
                }
                Ok(Message::Close(_)) => break,
                Err(_) => break,
                _ => {}
            }
        }
    };

    // Forward gorp -> client
    let gorp_to_client = async {
        while let Some(msg) = gorp_stream.next().await {
            match msg {
                Ok(tokio_tungstenite::tungstenite::Message::Text(text)) => {
                    if client_sink.send(Message::Text(text.into())).await.is_err() {
                        break;
                    }
                }
                Ok(tokio_tungstenite::tungstenite::Message::Binary(data)) => {
                    if client_sink.send(Message::Binary(data.into())).await.is_err() {
                        break;
                    }
                }
                Ok(tokio_tungstenite::tungstenite::Message::Close(_)) => break,
                Err(_) => break,
                _ => {}
            }
        }
    };

    tokio::select! {
        _ = client_to_gorp => {}
        _ = gorp_to_client => {}
    }
}
