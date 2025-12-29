//! CLI Bridge - WebSocket server for receiving events from CLI instances
//!
//! This module provides a WebSocket server that CLI instances can connect to
//! and send MCP traffic events. The events are then emitted to the Tauri frontend.

use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State,
    },
    response::IntoResponse,
    routing::get,
    Router,
};
use futures::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};
use tokio::sync::broadcast;
use tracing::{error, info, warn};

/// Event types that CLI can send to the GUI
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum CliEvent {
    /// A new session started
    #[serde(rename = "session_started")]
    SessionStarted {
        session_id: String,
        session_name: String,
        server_name: Option<String>,
    },
    /// A session ended
    #[serde(rename = "session_ended")]
    SessionEnded { session_id: String },
    /// A log entry (MCP message)
    #[serde(rename = "log")]
    Log {
        id: String,
        session_id: String,
        timestamp: u64,
        direction: String,
        content: String,
        method: Option<String>,
        server_name: Option<String>,
        /// Type of message content (jsonrpc, raw, stderr)
        #[serde(default)]
        message_type: Option<String>,
        /// Estimated token count for this message
        #[serde(default)]
        token_count: Option<u64>,
    },
}

/// State shared across WebSocket handlers
#[derive(Clone)]
pub struct CliBridgeState {
    pub app_handle: AppHandle,
    #[allow(dead_code)]
    pub shutdown_tx: broadcast::Sender<()>,
}

/// Start the CLI bridge WebSocket server
///
/// This creates a WebSocket server that CLI instances can connect to
/// for sending MCP traffic events to the GUI.
pub async fn start_cli_bridge(
    port: u16,
    app_handle: AppHandle,
) -> Result<(tokio::task::JoinHandle<()>, broadcast::Sender<()>), String> {
    info!("Starting CLI bridge WebSocket server on port {}", port);

    let (shutdown_tx, _) = broadcast::channel::<()>(1);

    let state = CliBridgeState {
        app_handle: app_handle.clone(),
        shutdown_tx: shutdown_tx.clone(),
    };

    let app = Router::new()
        .route("/ws", get(ws_handler))
        .route("/health", get(health_handler))
        .with_state(state);

    let addr = format!("127.0.0.1:{port}");
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .map_err(|e| format!("Failed to bind CLI bridge to {addr}: {e}"))?;

    info!("CLI bridge listening on ws://{}", addr);
    eprintln!("[CLI BRIDGE] WebSocket server listening on ws://{addr}/ws");

    // Emit event to frontend that bridge is ready
    let _ = app_handle.emit("cli-bridge-ready", serde_json::json!({ "port": port }));

    let shutdown_tx_clone = shutdown_tx.clone();
    let handle = tokio::spawn(async move {
        let mut shutdown_rx = shutdown_tx_clone.subscribe();

        tokio::select! {
            result = axum::serve(listener, app) => {
                if let Err(e) = result {
                    error!("CLI bridge server error: {}", e);
                }
            }
            _ = shutdown_rx.recv() => {
                info!("CLI bridge shutting down");
            }
        }
    });

    Ok((handle, shutdown_tx))
}

/// Health check endpoint
async fn health_handler() -> &'static str {
    "CLI Bridge is healthy"
}

/// WebSocket upgrade handler
async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<CliBridgeState>,
) -> impl IntoResponse {
    ws.on_upgrade(|socket| handle_socket(socket, state))
}

/// Handle an individual WebSocket connection from a CLI instance
async fn handle_socket(socket: WebSocket, state: CliBridgeState) {
    let (mut sender, mut receiver) = socket.split();

    info!("CLI instance connected");
    eprintln!("[CLI BRIDGE] New CLI instance connected");

    // Send welcome message
    let welcome = serde_json::json!({
        "type": "welcome",
        "message": "Connected to Reticle GUI"
    });
    if let Err(e) = sender
        .send(Message::Text(serde_json::to_string(&welcome).unwrap()))
        .await
    {
        error!("Failed to send welcome message: {}", e);
        return;
    }

    // Process incoming messages
    while let Some(msg) = receiver.next().await {
        match msg {
            Ok(Message::Text(text)) => {
                info!("Received from CLI: {}", &text[..text.len().min(200)]);
                eprintln!("[CLI BRIDGE] Received: {}", &text[..text.len().min(100)]);

                match serde_json::from_str::<CliEvent>(&text) {
                    Ok(event) => {
                        info!("Parsed CLI event: {:?}", event);
                        if let Err(e) = handle_cli_event(&state.app_handle, event).await {
                            warn!("Failed to handle CLI event: {}", e);
                        }
                    }
                    Err(e) => {
                        warn!(
                            "Failed to parse CLI event: {} - {}",
                            e,
                            &text[..text.len().min(100)]
                        );
                        eprintln!(
                            "[CLI BRIDGE] Parse error: {} - {}",
                            e,
                            &text[..text.len().min(100)]
                        );
                    }
                }
            }
            Ok(Message::Close(_)) => {
                info!("CLI instance disconnected");
                eprintln!("[CLI BRIDGE] CLI instance disconnected");
                break;
            }
            Ok(Message::Ping(data)) => {
                if let Err(e) = sender.send(Message::Pong(data)).await {
                    error!("Failed to send pong: {}", e);
                    break;
                }
            }
            Err(e) => {
                error!("WebSocket error: {}", e);
                break;
            }
            _ => {}
        }
    }
}

/// Handle a CLI event and emit it to the Tauri frontend
async fn handle_cli_event(app_handle: &AppHandle, event: CliEvent) -> Result<(), String> {
    match event {
        CliEvent::SessionStarted {
            session_id,
            session_name,
            server_name,
        } => {
            info!("CLI session started: {} ({})", session_name, session_id);
            eprintln!("[CLI BRIDGE] Session started: {session_name} ({session_id})");

            // Get current timestamp in microseconds
            let started_at = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_micros() as u64;

            // Emit in format expected by frontend: {id, started_at}
            app_handle
                .emit(
                    "session-start",
                    serde_json::json!({
                        "id": session_id,
                        "started_at": started_at,
                        "session_name": session_name,
                        "server_name": server_name,
                        "from_cli": true
                    }),
                )
                .map_err(|e| e.to_string())?;
        }
        CliEvent::SessionEnded { session_id } => {
            info!("CLI session ended: {}", session_id);

            app_handle
                .emit(
                    "session-end",
                    serde_json::json!({
                        "session_id": session_id,
                        "from_cli": true
                    }),
                )
                .map_err(|e| e.to_string())?;
        }
        CliEvent::Log {
            id,
            session_id,
            timestamp,
            direction,
            content,
            method,
            server_name,
            message_type,
            token_count,
        } => {
            info!(
                "CLI log: {} {} {:?}",
                direction,
                method.as_deref().unwrap_or("-"),
                &content[..content.len().min(50)]
            );
            eprintln!("[CLI BRIDGE] Emitting log-event: id={id}, direction={direction}, method={method:?}");

            let payload = serde_json::json!({
                "id": id,
                "session_id": session_id,
                "timestamp": timestamp,
                "direction": direction,
                "content": content,
                "method": method,
                "server_name": server_name,
                "message_type": message_type,
                "token_count": token_count,
                "from_cli": true
            });

            match app_handle.emit("log-event", payload) {
                Ok(_) => {
                    eprintln!("[CLI BRIDGE] log-event emitted successfully for {id}");
                }
                Err(e) => {
                    eprintln!("[CLI BRIDGE] ERROR emitting log-event for {id}: {e}");
                    return Err(e.to_string());
                }
            }
        }
    }

    Ok(())
}
