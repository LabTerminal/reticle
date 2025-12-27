//! WebSocket Transport Proxy
//!
//! Implements a WebSocket proxy for MCP servers that use WebSocket transport.
//! This provides true bidirectional real-time communication with lower latency
//! than HTTP-based transports.
//!
//! Key features:
//! - Full-duplex bidirectional communication
//! - Lower latency than HTTP polling
//! - Automatic reconnection handling
//! - Message interception and logging

use axum::http::{HeaderValue, Method};
use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State,
    },
    http::StatusCode,
    response::IntoResponse,
    routing::get,
    Router,
};
use futures_util::{SinkExt, StreamExt};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tauri::{AppHandle, Emitter};
use tokio::sync::{mpsc, Mutex, RwLock};
use tokio_tungstenite::{connect_async, tungstenite::Message as TungsteniteMessage};
use tower_http::cors::CorsLayer;
use tracing::{debug, error, info, warn};

use super::protocol::{Direction, LogEntry};
use super::session_recorder::{MessageDirection, SessionRecorder};

/// Global message counter for generating unique IDs
static WS_MESSAGE_COUNTER: AtomicU64 = AtomicU64::new(0);

/// State shared across WebSocket proxy handlers
#[derive(Clone)]
pub struct WebSocketProxyState {
    /// The upstream MCP server WebSocket URL
    pub server_url: String,
    /// The session ID for this proxy instance
    pub session_id: String,
    /// Tauri app handle for emitting events
    pub app_handle: AppHandle,
    /// Session recorder for capturing messages
    pub recorder: Arc<Mutex<Option<SessionRecorder>>>,
    /// Connection status
    pub is_connected: Arc<RwLock<bool>>,
}

/// Start the WebSocket proxy server
///
/// Creates an HTTP server with WebSocket upgrade support that acts as a
/// bidirectional proxy to the real MCP WebSocket server.
pub async fn start_websocket_proxy(
    server_url: String,
    proxy_port: u16,
    session_id: String,
    app_handle: AppHandle,
    recorder: Arc<Mutex<Option<SessionRecorder>>>,
) -> Result<tokio::task::JoinHandle<()>, String> {
    info!(
        "Starting WebSocket proxy on port {} -> {}",
        proxy_port, server_url
    );

    let state = WebSocketProxyState {
        server_url: server_url.clone(),
        session_id: session_id.clone(),
        app_handle: app_handle.clone(),
        recorder,
        is_connected: Arc::new(RwLock::new(false)),
    };

    // CORS layer - restricted to localhost origins for security
    let cors = CorsLayer::new()
        .allow_origin([
            "http://localhost".parse::<HeaderValue>().unwrap(),
            "http://127.0.0.1".parse::<HeaderValue>().unwrap(),
            "tauri://localhost".parse::<HeaderValue>().unwrap(),
            "ws://localhost".parse::<HeaderValue>().unwrap(),
            "ws://127.0.0.1".parse::<HeaderValue>().unwrap(),
        ])
        .allow_methods([Method::GET, Method::OPTIONS])
        .allow_headers(tower_http::cors::Any);

    // Create Axum router with WebSocket endpoint
    let app = Router::new()
        .route("/ws", get(ws_handler))
        .route("/health", get(health_handler))
        .with_state(state)
        .layer(cors);

    // Bind to localhost only for security (prevents external access)
    let addr = format!("127.0.0.1:{proxy_port}");
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .map_err(|e| format!("Failed to bind to {addr}: {e}"))?;

    eprintln!("[WEBSOCKET PROXY] Listening on ws://{addr}/ws");
    eprintln!("[WEBSOCKET PROXY] Proxying to {server_url}");
    info!("WebSocket proxy listening on {}", addr);

    // Spawn server in background
    let handle = tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, app).await {
            error!("WebSocket proxy server error: {}", e);
            eprintln!("[WEBSOCKET PROXY ERROR] Server error: {e}");
        }
    });

    Ok(handle)
}

/// Health check endpoint
async fn health_handler() -> (StatusCode, &'static str) {
    (StatusCode::OK, "WebSocket Proxy is healthy")
}

/// WebSocket upgrade handler
async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<WebSocketProxyState>,
) -> impl IntoResponse {
    debug!("WebSocket upgrade request received");
    eprintln!("[WEBSOCKET PROXY] Client connecting...");

    ws.on_upgrade(move |socket| handle_websocket(socket, state))
}

/// Handle the WebSocket connection
async fn handle_websocket(client_socket: WebSocket, state: WebSocketProxyState) {
    eprintln!("[WEBSOCKET PROXY] Client connected, connecting to upstream...");

    // Connect to upstream WebSocket server
    let upstream_result = connect_async(&state.server_url).await;

    let (upstream_ws, _response) = match upstream_result {
        Ok((ws, resp)) => {
            eprintln!("[WEBSOCKET PROXY] Connected to upstream server");
            (ws, resp)
        }
        Err(e) => {
            error!("Failed to connect to upstream WebSocket: {}", e);
            eprintln!("[WEBSOCKET PROXY ERROR] Failed to connect to upstream: {e}");

            // Emit error as a log event so it appears in the UI
            let error_id = generate_message_id();
            let error_json = serde_json::json!({
                "error": {
                    "code": -32000,
                    "message": "WebSocket connection failed",
                    "data": format!("{e}")
                }
            });
            let error_entry = LogEntry::new(
                error_id,
                state.session_id.clone(),
                Direction::Out,
                error_json,
            );
            if let Err(emit_err) = state.app_handle.emit("log-event", &error_entry) {
                warn!("Failed to emit WebSocket error log event: {}", emit_err);
            }

            return;
        }
    };

    // Update connection status
    {
        let mut connected = state.is_connected.write().await;
        *connected = true;
    }

    // Split both sockets into read/write halves
    let (mut client_write, mut client_read) = client_socket.split();
    let (mut upstream_write, mut upstream_read) = upstream_ws.split();

    // Create channels for message passing
    let (client_to_upstream_tx, mut client_to_upstream_rx) = mpsc::channel::<String>(100);
    let (upstream_to_client_tx, mut upstream_to_client_rx) = mpsc::channel::<String>(100);

    let session_id = state.session_id.clone();
    let app_handle = state.app_handle.clone();
    let recorder = state.recorder.clone();

    // Spawn task to read from client and send to upstream
    let session_id_clone = session_id.clone();
    let app_handle_clone = app_handle.clone();
    let recorder_clone = recorder.clone();
    let client_read_handle = tokio::spawn(async move {
        while let Some(msg_result) = client_read.next().await {
            match msg_result {
                Ok(Message::Text(text)) => {
                    eprintln!(
                        "[WEBSOCKET PROXY] Client → Server: {}",
                        &text[..text.len().min(100)]
                    );

                    // Log the message
                    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) {
                        let id = generate_message_id();
                        let entry = LogEntry::new(
                            id,
                            session_id_clone.clone(),
                            Direction::In,
                            json.clone(),
                        );

                        if let Err(e) = app_handle_clone.emit("log-event", &entry) {
                            warn!("Failed to emit log event: {}", e);
                        }

                        // Record message
                        let recorder_lock = recorder_clone.lock().await;
                        if let Some(ref rec) = *recorder_lock {
                            if let Err(e) =
                                rec.record_message(json, MessageDirection::ToServer).await
                            {
                                warn!("Failed to record message: {}", e);
                            }
                        }
                    }

                    // Forward to upstream
                    if client_to_upstream_tx.send(text).await.is_err() {
                        break;
                    }
                }
                Ok(Message::Binary(data)) => {
                    // Try to parse as text
                    if let Ok(text) = String::from_utf8(data.clone()) {
                        eprintln!(
                            "[WEBSOCKET PROXY] Client → Server (binary): {}",
                            &text[..text.len().min(100)]
                        );

                        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) {
                            let id = generate_message_id();
                            let entry = LogEntry::new(
                                id,
                                session_id_clone.clone(),
                                Direction::In,
                                json.clone(),
                            );

                            if let Err(e) = app_handle_clone.emit("log-event", &entry) {
                                warn!("Failed to emit log event: {}", e);
                            }
                        }

                        if client_to_upstream_tx.send(text).await.is_err() {
                            break;
                        }
                    }
                }
                Ok(Message::Close(_)) => {
                    eprintln!("[WEBSOCKET PROXY] Client closed connection");
                    break;
                }
                Ok(Message::Ping(_)) | Ok(Message::Pong(_)) => {
                    // Ignore ping/pong
                }
                Err(e) => {
                    error!("Error reading from client: {}", e);

                    // Emit error as a log event so it appears in the UI
                    let error_id = generate_message_id();
                    let error_json = serde_json::json!({
                        "error": {
                            "code": -32000,
                            "message": "Client read error",
                            "data": format!("{e}")
                        }
                    });
                    let error_entry = LogEntry::new(
                        error_id,
                        session_id_clone.clone(),
                        Direction::In,
                        error_json,
                    );
                    if let Err(emit_err) = app_handle_clone.emit("log-event", &error_entry) {
                        warn!("Failed to emit client error log event: {}", emit_err);
                    }

                    break;
                }
            }
        }
    });

    // Spawn task to read from upstream and send to client
    let session_id_clone = session_id.clone();
    let app_handle_clone = app_handle.clone();
    let recorder_clone = recorder.clone();
    let upstream_read_handle = tokio::spawn(async move {
        while let Some(msg_result) = upstream_read.next().await {
            match msg_result {
                Ok(TungsteniteMessage::Text(text)) => {
                    eprintln!(
                        "[WEBSOCKET PROXY] Server → Client: {}",
                        &text[..text.len().min(100)]
                    );

                    // Log the message
                    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) {
                        let id = generate_message_id();
                        let entry = LogEntry::new(
                            id,
                            session_id_clone.clone(),
                            Direction::Out,
                            json.clone(),
                        );

                        if let Err(e) = app_handle_clone.emit("log-event", &entry) {
                            warn!("Failed to emit log event: {}", e);
                        }

                        // Record message
                        let recorder_lock = recorder_clone.lock().await;
                        if let Some(ref rec) = *recorder_lock {
                            if let Err(e) =
                                rec.record_message(json, MessageDirection::ToClient).await
                            {
                                warn!("Failed to record message: {}", e);
                            }
                        }
                    }

                    // Forward to client
                    if upstream_to_client_tx.send(text).await.is_err() {
                        break;
                    }
                }
                Ok(TungsteniteMessage::Binary(data)) => {
                    if let Ok(text) = String::from_utf8(data.clone()) {
                        eprintln!(
                            "[WEBSOCKET PROXY] Server → Client (binary): {}",
                            &text[..text.len().min(100)]
                        );

                        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) {
                            let id = generate_message_id();
                            let entry = LogEntry::new(
                                id,
                                session_id_clone.clone(),
                                Direction::Out,
                                json.clone(),
                            );

                            if let Err(e) = app_handle_clone.emit("log-event", &entry) {
                                warn!("Failed to emit log event: {}", e);
                            }
                        }

                        if upstream_to_client_tx.send(text).await.is_err() {
                            break;
                        }
                    }
                }
                Ok(TungsteniteMessage::Close(_)) => {
                    eprintln!("[WEBSOCKET PROXY] Upstream closed connection");
                    break;
                }
                Ok(TungsteniteMessage::Ping(_)) | Ok(TungsteniteMessage::Pong(_)) => {
                    // Ignore ping/pong
                }
                Ok(TungsteniteMessage::Frame(_)) => {
                    // Raw frame, ignore
                }
                Err(e) => {
                    error!("Error reading from upstream: {}", e);

                    // Emit error as a log event so it appears in the UI
                    let error_id = generate_message_id();
                    let error_json = serde_json::json!({
                        "error": {
                            "code": -32000,
                            "message": "Upstream read error",
                            "data": format!("{e}")
                        }
                    });
                    let error_entry = LogEntry::new(
                        error_id,
                        session_id_clone.clone(),
                        Direction::Out,
                        error_json,
                    );
                    if let Err(emit_err) = app_handle_clone.emit("log-event", &error_entry) {
                        warn!("Failed to emit upstream error log event: {}", emit_err);
                    }

                    break;
                }
            }
        }
    });

    // Spawn task to write to upstream
    let upstream_write_handle = tokio::spawn(async move {
        while let Some(text) = client_to_upstream_rx.recv().await {
            if upstream_write
                .send(TungsteniteMessage::Text(text))
                .await
                .is_err()
            {
                break;
            }
        }
    });

    // Spawn task to write to client
    let client_write_handle = tokio::spawn(async move {
        while let Some(text) = upstream_to_client_rx.recv().await {
            if client_write.send(Message::Text(text)).await.is_err() {
                break;
            }
        }
    });

    // Wait for any task to complete (connection closed)
    tokio::select! {
        _ = client_read_handle => {
            eprintln!("[WEBSOCKET PROXY] Client read task ended");
        }
        _ = upstream_read_handle => {
            eprintln!("[WEBSOCKET PROXY] Upstream read task ended");
        }
        _ = upstream_write_handle => {
            eprintln!("[WEBSOCKET PROXY] Upstream write task ended");
        }
        _ = client_write_handle => {
            eprintln!("[WEBSOCKET PROXY] Client write task ended");
        }
    }

    // Update connection status
    {
        let mut connected = state.is_connected.write().await;
        *connected = false;
    }

    eprintln!("[WEBSOCKET PROXY] Connection closed");
}

/// Generate a unique message ID
fn generate_message_id() -> String {
    let counter = WS_MESSAGE_COUNTER.fetch_add(1, Ordering::SeqCst);
    format!("ws-msg-{counter}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_id_generation() {
        let id1 = generate_message_id();
        let id2 = generate_message_id();
        assert_ne!(id1, id2);
        assert!(id1.starts_with("ws-msg-"));
    }
}
