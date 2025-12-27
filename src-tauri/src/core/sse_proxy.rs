use axum::http::{HeaderValue, Method};
use axum::{
    extract::State,
    http::StatusCode,
    response::sse::{Event, Sse},
    routing::{get, post},
    Json, Router,
};
use futures::stream::{Stream, StreamExt};
use serde::{Deserialize, Serialize};
use std::convert::Infallible;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tauri::{AppHandle, Emitter};
use tokio::sync::Mutex;
use tower_http::cors::CorsLayer;
use tracing::{debug, error, info, warn};

use super::protocol::{Direction, LogEntry, MessageType};
use super::session_recorder::{MessageDirection, SessionRecorder};

/// Global message counter for generating unique IDs
static SSE_MESSAGE_COUNTER: AtomicU64 = AtomicU64::new(0);

/// State shared across SSE proxy handlers
#[derive(Clone)]
pub struct SseProxyState {
    pub server_url: String,
    pub session_id: String,
    pub app_handle: AppHandle,
    pub recorder: Arc<Mutex<Option<SessionRecorder>>>,
}

/// Start the SSE proxy server
///
/// This creates an HTTP server that acts as a reverse proxy to the real MCP server.
/// It intercepts Server-Sent Events, parses JSON-RPC messages, and emits them to the frontend.
pub async fn start_sse_proxy(
    server_url: String,
    proxy_port: u16,
    session_id: String,
    app_handle: AppHandle,
    recorder: Arc<Mutex<Option<SessionRecorder>>>,
) -> Result<tokio::task::JoinHandle<()>, String> {
    info!(
        "Starting SSE proxy on port {} -> {}",
        proxy_port, server_url
    );

    let state = SseProxyState {
        server_url: server_url.clone(),
        session_id: session_id.clone(),
        app_handle: app_handle.clone(),
        recorder,
    };

    // CORS layer - restricted to localhost origins for security
    let cors = CorsLayer::new()
        .allow_origin([
            "http://localhost".parse::<HeaderValue>().unwrap(),
            "http://127.0.0.1".parse::<HeaderValue>().unwrap(),
            "tauri://localhost".parse::<HeaderValue>().unwrap(),
        ])
        .allow_methods([Method::GET, Method::POST, Method::OPTIONS])
        .allow_headers(tower_http::cors::Any);

    // Create Axum router with both GET (receive) and POST (send) endpoints
    let app = Router::new()
        .route("/events", get(sse_proxy_handler))
        .route("/message", post(send_message_handler))
        .route("/health", get(health_handler))
        .with_state(state)
        .layer(cors);

    // Bind to localhost only for security (prevents external access)
    let addr = format!("127.0.0.1:{proxy_port}");
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .map_err(|e| format!("Failed to bind to {addr}: {e}"))?;

    eprintln!("[SSE PROXY] Listening on http://{addr}");
    eprintln!("[SSE PROXY] Proxying to {server_url}");
    info!("SSE proxy listening on {}", addr);

    // Spawn server in background
    let handle = tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, app).await {
            error!("SSE proxy server error: {}", e);
            eprintln!("[SSE PROXY ERROR] Server error: {e}");
        }
    });

    Ok(handle)
}

/// Health check endpoint
async fn health_handler() -> (StatusCode, &'static str) {
    (StatusCode::OK, "SSE Proxy is healthy")
}

/// SSE proxy handler - intercepts SSE events from real MCP server
async fn sse_proxy_handler(
    State(state): State<SseProxyState>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, (StatusCode, String)> {
    debug!("SSE client connected");
    eprintln!("[SSE PROXY] Client connected");

    // Connect to real MCP server SSE endpoint
    let url = format!("{}/events", state.server_url);
    let client = reqwest::Client::new();

    let response = client.get(&url).send().await.map_err(|e| {
        error!("Failed to connect to MCP server at {}: {}", url, e);
        (
            StatusCode::BAD_GATEWAY,
            format!("Failed to connect to MCP server: {e}"),
        )
    })?;

    if !response.status().is_success() {
        error!("MCP server returned error status: {}", response.status());
        return Err((
            StatusCode::BAD_GATEWAY,
            format!("MCP server error: {}", response.status()),
        ));
    }

    debug!("Connected to MCP server at {}", url);
    eprintln!("[SSE PROXY] Connected to MCP server");

    // Stream SSE events
    let session_id = state.session_id.clone();
    let app_handle = state.app_handle.clone();
    let recorder = state.recorder.clone();

    let stream = response.bytes_stream().map(move |chunk_result| {
        match chunk_result {
            Ok(chunk) => {
                let data = String::from_utf8_lossy(&chunk);

                // Parse SSE event format
                if let Some(json_str) = parse_sse_data(&data) {
                    eprintln!(
                        "[SSE PROXY DEBUG] Received data: {}",
                        &json_str[..json_str.len().min(100)]
                    );

                    let id = generate_sse_message_id();

                    // Try to parse as JSON-RPC
                    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&json_str) {
                        debug!("Parsed JSON-RPC message");

                        // Create log entry
                        let entry = LogEntry::new(
                            id,
                            session_id.clone(),
                            Direction::Out, // SSE is server → client (outgoing)
                            json.clone(),
                        );

                        // Emit to frontend
                        if let Err(e) = app_handle.emit("log-event", &entry) {
                            warn!("Failed to emit log event: {}", e);
                            eprintln!("[SSE PROXY ERROR] Failed to emit: {e}");
                        } else {
                            debug!("Emitted log event: {}", entry.id);
                            eprintln!("[SSE PROXY DEBUG] Emitted log-event: {}", entry.id);
                        }

                        // Record message if recording is active
                        let recorder_clone = recorder.clone();
                        let json_clone = json.clone();
                        tokio::spawn(async move {
                            let recorder_lock = recorder_clone.lock().await;
                            if let Some(ref rec) = *recorder_lock {
                                if let Err(e) = rec
                                    .record_message(json_clone, MessageDirection::ToClient)
                                    .await
                                {
                                    warn!("Failed to record SSE message: {}", e);
                                }
                            }
                        });

                        // Forward to client
                        return Ok(Event::default().data(json_str));
                    } else {
                        // Non-JSON data - emit as raw message for debugging
                        eprintln!(
                            "[SSE PROXY DEBUG] Non-JSON SSE data, emitting as raw: {}",
                            &json_str[..json_str.len().min(100)]
                        );
                        debug!("Non-JSON data in SSE event: {}", json_str);

                        let entry = LogEntry::new_raw(
                            id,
                            session_id.clone(),
                            Direction::Out,
                            json_str.clone(),
                            MessageType::Raw,
                        );

                        // Emit to frontend
                        if let Err(e) = app_handle.emit("log-event", &entry) {
                            warn!("Failed to emit raw log event: {}", e);
                            eprintln!("[SSE PROXY ERROR] Failed to emit raw: {e}");
                        } else {
                            eprintln!("[SSE PROXY DEBUG] Emitted raw log-event: {}", entry.id);
                        }

                        // Forward to client
                        return Ok(Event::default().data(json_str));
                    }
                }

                // Forward raw event if not JSON-RPC (no data: prefix found)
                Ok(Event::default().data(&data))
            }
            Err(e) => {
                error!("Error reading SSE stream: {}", e);
                Ok(Event::default().comment(format!("error: {e}")))
            }
        }
    });

    Ok(Sse::new(stream))
}

/// Request body for POST /message endpoint
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendMessageRequest {
    /// The JSON-RPC message to send
    pub message: serde_json::Value,
}

/// Response from POST /message endpoint
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendMessageResponse {
    /// Whether the message was sent successfully
    pub success: bool,
    /// The response from the MCP server (if any)
    pub response: Option<serde_json::Value>,
    /// Error message if failed
    pub error: Option<String>,
}

/// POST handler for sending messages to the MCP server
///
/// This endpoint forwards JSON-RPC requests to the real MCP server
/// and returns the response.
async fn send_message_handler(
    State(state): State<SseProxyState>,
    Json(request): Json<SendMessageRequest>,
) -> Result<Json<SendMessageResponse>, (StatusCode, String)> {
    debug!("Sending message to MCP server: {:?}", request.message);
    eprintln!(
        "[SSE PROXY] Sending message: {}",
        serde_json::to_string(&request.message).unwrap_or_default()
    );

    // Log the outgoing request
    let id = generate_sse_message_id();
    let entry = LogEntry::new(
        id.clone(),
        state.session_id.clone(),
        Direction::In, // Direction::In means we sent it TO the server
        request.message.clone(),
    );

    // Emit to frontend so user sees their sent request
    if let Err(e) = state.app_handle.emit("log-event", &entry) {
        warn!("Failed to emit sent request: {}", e);
    }

    // Record if recording is active
    {
        let recorder_lock = state.recorder.lock().await;
        if let Some(ref rec) = *recorder_lock {
            if let Err(e) = rec
                .record_message(request.message.clone(), MessageDirection::ToServer)
                .await
            {
                warn!("Failed to record sent message: {}", e);
            }
        }
    }

    // Forward to MCP server
    let client = reqwest::Client::new();

    // Try POST to /message endpoint first (MCP HTTP transport standard)
    let response = client
        .post(format!("{}/message", state.server_url))
        .json(&request.message)
        .send()
        .await;

    match response {
        Ok(resp) => {
            if resp.status().is_success() {
                // Try to parse response body as JSON
                // Note: We do NOT emit the response here because it will come through the SSE stream
                // This prevents duplicate messages when the server sends responses via SSE
                match resp.json::<serde_json::Value>().await {
                    Ok(json_response) => {
                        eprintln!(
                            "[SSE PROXY] Got response (not emitting - will come via SSE): {}",
                            serde_json::to_string(&json_response).unwrap_or_default()
                        );

                        Ok(Json(SendMessageResponse {
                            success: true,
                            response: Some(json_response),
                            error: None,
                        }))
                    }
                    Err(e) => {
                        // Response wasn't JSON, but request succeeded
                        eprintln!("[SSE PROXY] Response not JSON: {e}");
                        Ok(Json(SendMessageResponse {
                            success: true,
                            response: None,
                            error: None,
                        }))
                    }
                }
            } else {
                let status = resp.status();
                let error_text = resp
                    .text()
                    .await
                    .unwrap_or_else(|_| "Unknown error".to_string());
                eprintln!("[SSE PROXY] Server error: {status} - {error_text}");

                // Emit error as a log event so it appears in the UI
                let error_id = generate_sse_message_id();
                let error_json = serde_json::json!({
                    "error": {
                        "code": status.as_u16(),
                        "message": format!("Server returned {status}"),
                        "data": error_text
                    }
                });
                let error_entry = LogEntry::new(
                    error_id,
                    state.session_id.clone(),
                    Direction::Out,
                    error_json,
                );
                if let Err(e) = state.app_handle.emit("log-event", &error_entry) {
                    warn!("Failed to emit error log event: {}", e);
                }

                Ok(Json(SendMessageResponse {
                    success: false,
                    response: None,
                    error: Some(format!("Server returned {status}: {error_text}")),
                }))
            }
        }
        Err(e) => {
            error!("Failed to send message to MCP server: {}", e);
            eprintln!("[SSE PROXY ERROR] Failed to send: {e}");

            // Emit connection error as a log event so it appears in the UI
            let error_id = generate_sse_message_id();
            let error_json = serde_json::json!({
                "error": {
                    "code": -32000,
                    "message": "Connection failed",
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
                warn!("Failed to emit connection error log event: {}", emit_err);
            }

            Ok(Json(SendMessageResponse {
                success: false,
                response: None,
                error: Some(format!("Failed to connect to MCP server: {e}")),
            }))
        }
    }
}

/// Parse SSE event data field
///
/// SSE format:
///   event: <type>
///   id: <id>
///   data: <content>
///   <blank line>
///
/// We're looking for lines starting with "data: " and extracting the content
fn parse_sse_data(event_str: &str) -> Option<String> {
    for line in event_str.lines() {
        if let Some(data) = line.strip_prefix("data: ") {
            let trimmed = data.trim();
            if !trimmed.is_empty() && trimmed != ":" {
                return Some(trimmed.to_string());
            }
        }
    }
    None
}

/// Generate a unique message ID for SSE messages
fn generate_sse_message_id() -> String {
    let counter = SSE_MESSAGE_COUNTER.fetch_add(1, Ordering::SeqCst);
    format!("sse-msg-{counter}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_sse_data() {
        // Standard SSE event
        let event = "data: {\"jsonrpc\":\"2.0\",\"id\":1}\n\n";
        assert_eq!(
            parse_sse_data(event),
            Some("{\"jsonrpc\":\"2.0\",\"id\":1}".to_string())
        );

        // With event type
        let event_with_type = "event: message\ndata: {\"test\":true}\n\n";
        assert_eq!(
            parse_sse_data(event_with_type),
            Some("{\"test\":true}".to_string())
        );

        // Comment only
        let comment = ": heartbeat\n\n";
        assert_eq!(parse_sse_data(comment), None);

        // Empty
        assert_eq!(parse_sse_data(""), None);
    }
}
