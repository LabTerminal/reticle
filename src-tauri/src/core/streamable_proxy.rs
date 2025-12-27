//! Streamable HTTP Transport Proxy
//!
//! Implements the MCP Streamable HTTP Transport (protocol version 2025-03-26).
//! This transport uses bidirectional HTTP with optional SSE streaming.
//!
//! Key features:
//! - Single MCP endpoint supporting POST and GET methods
//! - Session management via Mcp-Session-Id header
//! - Supports both JSON responses and SSE streaming
//! - Backwards compatible detection for legacy HTTP+SSE transport

use axum::http::{HeaderValue, Method};
use axum::{
    body::Body,
    extract::State,
    http::StatusCode,
    response::{
        sse::{Event, Sse},
        IntoResponse, Response,
    },
    routing::{delete, get, post},
    Json, Router,
};
use futures::stream::StreamExt;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tauri::{AppHandle, Emitter};
use tokio::sync::{Mutex, RwLock};
use tower_http::cors::CorsLayer;
use tracing::{debug, error, info, warn};

use super::protocol::{Direction, LogEntry};
use super::session_recorder::{MessageDirection, SessionRecorder};

/// Global message counter for generating unique IDs
static STREAMABLE_MESSAGE_COUNTER: AtomicU64 = AtomicU64::new(0);

/// MCP Session ID header name
const MCP_SESSION_ID_HEADER: &str = "mcp-session-id";

/// State shared across Streamable HTTP proxy handlers
#[derive(Clone)]
pub struct StreamableProxyState {
    /// The upstream MCP server URL
    pub server_url: String,
    /// The session ID for this proxy instance
    pub session_id: String,
    /// Tauri app handle for emitting events
    pub app_handle: AppHandle,
    /// Session recorder for capturing messages
    pub recorder: Arc<Mutex<Option<SessionRecorder>>>,
    /// HTTP client for making requests to upstream server
    pub client: Client,
    /// Active MCP session ID from upstream server (if assigned)
    pub mcp_session_id: Arc<RwLock<Option<String>>>,
    /// SSE event ID counter for resumability
    pub event_counter: Arc<AtomicU64>,
}

/// Response wrapper for JSON-RPC messages
#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
pub struct JsonRpcMessage {
    pub jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub method: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<serde_json::Value>,
}

/// Start the Streamable HTTP proxy server
///
/// Creates an HTTP server that acts as a reverse proxy to the real MCP server,
/// implementing the MCP Streamable HTTP Transport (2025-03-26 spec).
pub async fn start_streamable_proxy(
    server_url: String,
    proxy_port: u16,
    session_id: String,
    app_handle: AppHandle,
    recorder: Arc<Mutex<Option<SessionRecorder>>>,
) -> Result<tokio::task::JoinHandle<()>, String> {
    info!(
        "Starting Streamable HTTP proxy on port {} -> {}",
        proxy_port, server_url
    );

    let client = Client::builder()
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {e}"))?;

    let state = StreamableProxyState {
        server_url: server_url.clone(),
        session_id: session_id.clone(),
        app_handle: app_handle.clone(),
        recorder,
        client,
        mcp_session_id: Arc::new(RwLock::new(None)),
        event_counter: Arc::new(AtomicU64::new(0)),
    };

    // CORS layer - restricted to localhost origins for security
    let cors = CorsLayer::new()
        .allow_origin([
            "http://localhost".parse::<HeaderValue>().unwrap(),
            "http://127.0.0.1".parse::<HeaderValue>().unwrap(),
            "tauri://localhost".parse::<HeaderValue>().unwrap(),
        ])
        .allow_methods([Method::GET, Method::POST, Method::DELETE, Method::OPTIONS])
        .allow_headers(tower_http::cors::Any);

    // Create Axum router with MCP endpoint
    // The single /mcp endpoint handles POST, GET, and DELETE
    let app = Router::new()
        .route("/mcp", post(handle_post))
        .route("/mcp", get(handle_get))
        .route("/mcp", delete(handle_delete))
        .route("/health", get(health_handler))
        // Legacy compatibility endpoints
        .route("/message", post(handle_post))
        .route("/events", get(handle_get))
        .with_state(state)
        .layer(cors);

    // Bind to localhost only for security (prevents external access)
    let addr = format!("127.0.0.1:{proxy_port}");
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .map_err(|e| format!("Failed to bind to {addr}: {e}"))?;

    eprintln!("[STREAMABLE PROXY] Listening on http://{addr}");
    eprintln!("[STREAMABLE PROXY] Proxying to {server_url}");
    info!("Streamable HTTP proxy listening on {}", addr);

    // Spawn server in background
    let handle = tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, app).await {
            error!("Streamable HTTP proxy server error: {}", e);
            eprintln!("[STREAMABLE PROXY ERROR] Server error: {e}");
        }
    });

    Ok(handle)
}

/// Health check endpoint
async fn health_handler() -> (StatusCode, &'static str) {
    (StatusCode::OK, "Streamable HTTP Proxy is healthy")
}

/// Extract Mcp-Session-Id from axum headers
fn extract_session_id(headers: &axum::http::HeaderMap) -> Option<String> {
    headers
        .get(MCP_SESSION_ID_HEADER)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
}

/// Handle POST requests to the MCP endpoint
///
/// This is the main endpoint for sending JSON-RPC messages to the server.
/// According to the spec:
/// - Accepts single JSON-RPC message or batched array
/// - Returns 202 Accepted for notifications/responses only
/// - Returns JSON or SSE stream for requests
async fn handle_post(
    State(state): State<StreamableProxyState>,
    headers: axum::http::HeaderMap,
    body: String,
) -> Response {
    debug!("Received POST request");
    eprintln!("[STREAMABLE PROXY] POST: {}", &body[..body.len().min(200)]);

    // Parse the incoming JSON-RPC message(s)
    let messages: Vec<serde_json::Value> = match serde_json::from_str::<serde_json::Value>(&body) {
        Ok(serde_json::Value::Array(arr)) => arr,
        Ok(val) => vec![val],
        Err(e) => {
            error!("Failed to parse JSON-RPC message: {}", e);
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "jsonrpc": "2.0",
                    "error": {
                        "code": -32700,
                        "message": format!("Parse error: {}", e)
                    }
                })),
            )
                .into_response();
        }
    };

    // Log incoming messages
    for msg in &messages {
        let id = generate_message_id();
        let entry = LogEntry::new(id, state.session_id.clone(), Direction::In, msg.clone());
        if let Err(e) = state.app_handle.emit("log-event", &entry) {
            warn!("Failed to emit log event: {}", e);
        }

        // Record if recording is active
        let recorder_clone = state.recorder.clone();
        let msg_clone = msg.clone();
        tokio::spawn(async move {
            let recorder_lock = recorder_clone.lock().await;
            if let Some(ref rec) = *recorder_lock {
                if let Err(e) = rec
                    .record_message(msg_clone, MessageDirection::ToServer)
                    .await
                {
                    warn!("Failed to record message: {}", e);
                }
            }
        });
    }

    // Build upstream request using reqwest types
    let mut request = state
        .client
        .post(format!("{}/mcp", state.server_url.trim_end_matches('/')))
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .header(
            reqwest::header::ACCEPT,
            "application/json, text/event-stream",
        );

    // Forward Mcp-Session-Id if present
    if let Some(session_id) = extract_session_id(&headers) {
        request = request.header(MCP_SESSION_ID_HEADER, session_id);
    } else {
        // Check if we have a stored session ID
        let stored_session = state.mcp_session_id.read().await;
        if let Some(ref sid) = *stored_session {
            request = request.header(MCP_SESSION_ID_HEADER, sid.clone());
        }
    }

    // Send to upstream server
    let response = request.body(body.clone()).send().await;

    match response {
        Ok(resp) => {
            let status = resp.status();
            let resp_headers = resp.headers().clone();

            // Capture Mcp-Session-Id from response
            if let Some(session_id) = resp_headers.get(MCP_SESSION_ID_HEADER) {
                if let Ok(sid) = session_id.to_str() {
                    let mut stored = state.mcp_session_id.write().await;
                    *stored = Some(sid.to_string());
                    eprintln!("[STREAMABLE PROXY] Captured session ID: {sid}");
                }
            }

            // Check content type
            let content_type = resp_headers
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .unwrap_or("");

            if content_type.contains("text/event-stream") {
                // Stream SSE response
                eprintln!("[STREAMABLE PROXY] Streaming SSE response");
                handle_sse_response(state, resp).await
            } else if status == reqwest::StatusCode::ACCEPTED {
                // 202 Accepted - no body expected
                eprintln!("[STREAMABLE PROXY] Got 202 Accepted");
                StatusCode::ACCEPTED.into_response()
            } else {
                // JSON response
                match resp.text().await {
                    Ok(text) => {
                        eprintln!(
                            "[STREAMABLE PROXY] JSON response: {}",
                            &text[..text.len().min(200)]
                        );

                        // Log response
                        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) {
                            log_outgoing_message(&state, json.clone()).await;
                        }

                        let axum_status =
                            StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::OK);

                        let mut response = Response::builder()
                            .status(axum_status)
                            .header("content-type", "application/json");

                        // Forward session ID if present
                        if let Some(sid) = resp_headers.get(MCP_SESSION_ID_HEADER) {
                            if let Ok(s) = sid.to_str() {
                                response = response.header(MCP_SESSION_ID_HEADER, s);
                            }
                        }

                        response
                            .body(Body::from(text))
                            .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
                    }
                    Err(e) => {
                        error!("Failed to read response body: {}", e);
                        (
                            StatusCode::BAD_GATEWAY,
                            format!("Failed to read upstream response: {e}"),
                        )
                            .into_response()
                    }
                }
            }
        }
        Err(e) => {
            error!("Failed to connect to upstream server: {}", e);
            eprintln!("[STREAMABLE PROXY ERROR] Upstream error: {e}");

            // Emit error as a log event so it appears in the UI
            let error_id = generate_message_id();
            let error_json = serde_json::json!({
                "jsonrpc": "2.0",
                "error": {
                    "code": -32603,
                    "message": "Connection failed",
                    "data": format!("{e}")
                }
            });
            let error_entry = LogEntry::new(
                error_id,
                state.session_id.clone(),
                Direction::Out,
                error_json.clone(),
            );
            if let Err(emit_err) = state.app_handle.emit("log-event", &error_entry) {
                warn!("Failed to emit connection error log event: {}", emit_err);
            }

            (
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({
                    "jsonrpc": "2.0",
                    "error": {
                        "code": -32603,
                        "message": format!("Failed to connect to MCP server: {}", e)
                    }
                })),
            )
                .into_response()
        }
    }
}

/// Handle SSE streaming response from upstream
async fn handle_sse_response(state: StreamableProxyState, response: reqwest::Response) -> Response {
    let session_id = state.session_id.clone();
    let app_handle = state.app_handle.clone();
    let recorder = state.recorder.clone();
    let event_counter = state.event_counter.clone();

    let stream = response.bytes_stream().map(move |chunk_result| {
        match chunk_result {
            Ok(chunk) => {
                let data = String::from_utf8_lossy(&chunk);

                // Parse SSE events
                for line in data.lines() {
                    if let Some(json_str) = line.strip_prefix("data: ") {
                        if json_str.trim().is_empty() {
                            continue;
                        }

                        let _event_id = event_counter.fetch_add(1, Ordering::SeqCst);

                        if let Ok(json) = serde_json::from_str::<serde_json::Value>(json_str) {
                            let msg_id = generate_message_id();
                            let entry = LogEntry::new(
                                msg_id,
                                session_id.clone(),
                                Direction::Out,
                                json.clone(),
                            );

                            if let Err(e) = app_handle.emit("log-event", &entry) {
                                warn!("Failed to emit log event: {}", e);
                            }

                            // Record message
                            let recorder_clone = recorder.clone();
                            let json_clone = json.clone();
                            tokio::spawn(async move {
                                let recorder_lock = recorder_clone.lock().await;
                                if let Some(ref rec) = *recorder_lock {
                                    if let Err(e) = rec
                                        .record_message(json_clone, MessageDirection::ToClient)
                                        .await
                                    {
                                        warn!("Failed to record message: {}", e);
                                    }
                                }
                            });
                        }
                    }
                }

                Ok::<_, std::convert::Infallible>(Event::default().data(&data))
            }
            Err(e) => {
                error!("Error reading SSE stream: {}", e);
                Ok(Event::default().comment(format!("error: {e}")))
            }
        }
    });

    Sse::new(stream).into_response()
}

/// Handle GET requests to the MCP endpoint
///
/// Opens an SSE stream for server-initiated messages.
async fn handle_get(
    State(state): State<StreamableProxyState>,
    headers: axum::http::HeaderMap,
) -> Response {
    debug!("Received GET request for SSE stream");
    eprintln!("[STREAMABLE PROXY] GET: Opening SSE stream");

    // Build upstream request using reqwest
    let mut request = state
        .client
        .get(format!("{}/mcp", state.server_url.trim_end_matches('/')))
        .header(reqwest::header::ACCEPT, "text/event-stream");

    // Forward Mcp-Session-Id if present
    if let Some(session_id) = extract_session_id(&headers) {
        request = request.header(MCP_SESSION_ID_HEADER, session_id);
    } else {
        let stored_session = state.mcp_session_id.read().await;
        if let Some(ref sid) = *stored_session {
            request = request.header(MCP_SESSION_ID_HEADER, sid.clone());
        }
    }

    // Forward Last-Event-ID for resumability
    if let Some(last_event_id) = headers.get("last-event-id") {
        if let Ok(val) = last_event_id.to_str() {
            request = request.header("last-event-id", val);
        }
    }

    let response = request.send().await;

    match response {
        Ok(resp) => {
            if resp.status() == reqwest::StatusCode::METHOD_NOT_ALLOWED {
                eprintln!("[STREAMABLE PROXY] Server doesn't support SSE streams");
                return StatusCode::METHOD_NOT_ALLOWED.into_response();
            }

            handle_sse_response(state, resp).await
        }
        Err(e) => {
            error!("Failed to connect to upstream SSE: {}", e);

            // Emit error as a log event so it appears in the UI
            let error_id = generate_message_id();
            let error_json = serde_json::json!({
                "error": {
                    "code": -32000,
                    "message": "SSE connection failed",
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
                warn!("Failed to emit SSE error log event: {}", emit_err);
            }

            (
                StatusCode::BAD_GATEWAY,
                format!("Failed to connect to upstream SSE: {e}"),
            )
                .into_response()
        }
    }
}

/// Handle DELETE requests to the MCP endpoint
///
/// Client-initiated session termination.
async fn handle_delete(
    State(state): State<StreamableProxyState>,
    headers: axum::http::HeaderMap,
) -> Response {
    debug!("Received DELETE request for session termination");
    eprintln!("[STREAMABLE PROXY] DELETE: Session termination");

    // Build upstream request
    let mut request = state
        .client
        .delete(format!("{}/mcp", state.server_url.trim_end_matches('/')));

    // Forward Mcp-Session-Id
    if let Some(session_id) = extract_session_id(&headers) {
        request = request.header(MCP_SESSION_ID_HEADER, session_id);
    } else {
        let stored_session = state.mcp_session_id.read().await;
        if let Some(ref sid) = *stored_session {
            request = request.header(MCP_SESSION_ID_HEADER, sid.clone());
        }
    }

    let response = request.send().await;

    match response {
        Ok(resp) => {
            let status = resp.status();
            eprintln!("[STREAMABLE PROXY] DELETE response: {status}");

            // Clear stored session
            let mut stored = state.mcp_session_id.write().await;
            *stored = None;

            let axum_status = StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::OK);
            axum_status.into_response()
        }
        Err(e) => {
            error!("Failed to send DELETE to upstream: {}", e);

            // Emit error as a log event so it appears in the UI
            let error_id = generate_message_id();
            let error_json = serde_json::json!({
                "error": {
                    "code": -32000,
                    "message": "Session termination failed",
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
                warn!("Failed to emit DELETE error log event: {}", emit_err);
            }

            (
                StatusCode::BAD_GATEWAY,
                format!("Failed to terminate session: {e}"),
            )
                .into_response()
        }
    }
}

/// Log an outgoing message
async fn log_outgoing_message(state: &StreamableProxyState, json: serde_json::Value) {
    let id = generate_message_id();
    let entry = LogEntry::new(id, state.session_id.clone(), Direction::Out, json.clone());

    if let Err(e) = state.app_handle.emit("log-event", &entry) {
        warn!("Failed to emit log event: {}", e);
    }

    // Record if recording is active
    let recorder_lock = state.recorder.lock().await;
    if let Some(ref rec) = *recorder_lock {
        if let Err(e) = rec.record_message(json, MessageDirection::ToClient).await {
            warn!("Failed to record message: {}", e);
        }
    }
}

/// Generate a unique message ID
fn generate_message_id() -> String {
    let counter = STREAMABLE_MESSAGE_COUNTER.fetch_add(1, Ordering::SeqCst);
    format!("streamable-msg-{counter}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_id_generation() {
        let id1 = generate_message_id();
        let id2 = generate_message_id();
        assert_ne!(id1, id2);
        assert!(id1.starts_with("streamable-msg-"));
    }
}
