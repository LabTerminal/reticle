//! HTTP Proxy for CLI
//!
//! Implements an HTTP reverse proxy that intercepts MCP traffic and streams
//! telemetry to the Reticle GUI via Unix socket.
//!
//! This enables debugging of HTTP-based MCP servers (SSE, Streamable HTTP, WebSocket)
//! in the same hub-and-spoke architecture as stdio servers.

use axum::{
    body::Body,
    extract::{
        ws::{Message as AxumWsMessage, WebSocket, WebSocketUpgrade},
        State,
    },
    http::{Method, Request, StatusCode},
    response::{IntoResponse, Response},
    routing::{any, get},
    Router,
};
use bytes::Bytes;
use futures::{SinkExt, StreamExt};
use reqwest::Client;
use reticle_core::events::{NoOpEventSink, UnixSocketEventSink};
use reticle_core::protocol::{Direction, LogEntry, MessageType};
use reticle_core::session_names::{create_session_id, SessionId};
use reticle_core::token_counter::TokenCounter as TC;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio_tungstenite::{connect_async, tungstenite::Message as TungsteniteMessage};
use tower_http::cors::CorsLayer;
use tracing::{debug, error, info, warn};

/// Global message counter for generating unique IDs
static HTTP_MESSAGE_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Generate a unique message ID
fn generate_message_id() -> String {
    let count = HTTP_MESSAGE_COUNTER.fetch_add(1, Ordering::SeqCst);
    format!("http-{count}")
}

/// Event sink enum for HTTP proxy - allows Clone without dyn trait
#[derive(Clone)]
pub enum HttpEventSink {
    NoOp(NoOpEventSink),
    UnixSocket(Arc<UnixSocketEventSink>),
}

impl HttpEventSink {
    async fn emit_log(&self, entry: &LogEntry) -> Result<(), String> {
        use reticle_core::events::EventSink;
        match self {
            HttpEventSink::NoOp(sink) => sink.emit_log(entry).await,
            HttpEventSink::UnixSocket(sink) => sink.emit_log(entry).await,
        }
    }

    async fn emit_session_started(
        &self,
        session_id: &str,
        session_name: &str,
    ) -> Result<(), String> {
        use reticle_core::events::EventSink;
        match self {
            HttpEventSink::NoOp(sink) => sink.emit_session_started(session_id, session_name).await,
            HttpEventSink::UnixSocket(sink) => {
                sink.emit_session_started(session_id, session_name).await
            }
        }
    }

    async fn emit_session_ended(&self, session_id: &str) -> Result<(), String> {
        use reticle_core::events::EventSink;
        match self {
            HttpEventSink::NoOp(sink) => sink.emit_session_ended(session_id).await,
            HttpEventSink::UnixSocket(sink) => sink.emit_session_ended(session_id).await,
        }
    }
}

/// State shared across HTTP proxy handlers
#[derive(Clone)]
pub struct HttpProxyState {
    /// The upstream MCP server URL
    pub upstream_url: String,
    /// Session identifier (internal UUID + display name)
    pub session: SessionId,
    /// Server name for identification
    pub server_name: String,
    /// HTTP client for making requests to upstream server
    pub client: Client,
    /// Event sink for streaming telemetry
    pub event_sink: HttpEventSink,
    /// Inject receiver for GUI → proxy communication (future use)
    #[allow(dead_code)]
    pub inject_tx: Arc<Mutex<Option<tokio::sync::mpsc::Sender<String>>>>,
}

/// Run the HTTP proxy
///
/// Creates an HTTP server that acts as a reverse proxy to the real MCP server,
/// intercepting all traffic and streaming it to the GUI.
pub async fn run_http_proxy(
    upstream_url: String,
    listen_port: u16,
    server_name: String,
    event_sink: HttpEventSink,
    mut inject_rx: Option<tokio::sync::mpsc::Receiver<String>>,
) -> Result<(), String> {
    // Generate session ID with beautiful name
    let session = create_session_id(Some(&server_name));

    info!(
        "Starting HTTP proxy '{}' on port {} -> {}",
        session.name, listen_port, upstream_url
    );

    // Emit session started (use display name for UI, internal ID for tracking)
    event_sink
        .emit_session_started(&session.id, &session.name)
        .await
        .map_err(|e| format!("Failed to emit session started: {e}"))?;

    let client = Client::builder()
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {e}"))?;

    // Create channel for inject commands
    let (inject_tx, mut _proxy_inject_rx) = tokio::sync::mpsc::channel::<String>(100);

    let state = HttpProxyState {
        upstream_url: upstream_url.clone(),
        session: session.clone(),
        server_name: server_name.clone(),
        client,
        event_sink,
        inject_tx: Arc::new(Mutex::new(Some(inject_tx))),
    };

    // CORS layer - allow all for proxy
    let cors = CorsLayer::new()
        .allow_origin(tower_http::cors::Any)
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PUT,
            Method::DELETE,
            Method::OPTIONS,
        ])
        .allow_headers(tower_http::cors::Any);

    // Create router - catch all routes
    let app = Router::new()
        .route("/health", get(health_handler))
        .route("/", any(proxy_handler))
        .route("/*path", any(proxy_handler))
        .with_state(state.clone())
        .layer(cors);

    // Bind to localhost
    let addr = format!("127.0.0.1:{listen_port}");
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .map_err(|e| format!("Failed to bind to {addr}: {e}"))?;

    eprintln!("[HTTP PROXY] Session: {}", session.name);
    eprintln!("[HTTP PROXY] Listening on http://{addr}");
    eprintln!("[HTTP PROXY] Proxying to {upstream_url}");
    info!("HTTP proxy '{}' listening on {}", session.name, addr);

    // Forward inject commands from socket to the proxy state
    let _state_for_inject = state.clone();
    if inject_rx.is_some() {
        tokio::spawn(async move {
            if let Some(ref mut rx) = inject_rx {
                while let Some(message) = rx.recv().await {
                    info!("Received inject command: {} bytes", message.len());
                    // The inject message needs to be sent as an HTTP request
                    // For now, we'll queue it and the next request will pick it up
                    // TODO: Implement proper inject handling for HTTP
                }
            }
        });
    }

    // Run the server
    axum::serve(listener, app)
        .await
        .map_err(|e| format!("Server error: {e}"))?;

    // Emit session ended
    state
        .event_sink
        .emit_session_ended(&state.session.id)
        .await
        .map_err(|e| format!("Failed to emit session ended: {e}"))?;

    Ok(())
}

/// Health check endpoint
async fn health_handler() -> (StatusCode, &'static str) {
    (StatusCode::OK, "HTTP Proxy is healthy")
}

/// Main proxy handler - forwards all requests to upstream
async fn proxy_handler(
    State(state): State<HttpProxyState>,
    ws: Option<WebSocketUpgrade>,
    req: Request<Body>,
) -> Response {
    let method = req.method().clone();
    let uri = req.uri().clone();
    let headers = req.headers().clone();
    let path = uri.path();
    let query = uri.query().map(|q| format!("?{q}")).unwrap_or_default();

    debug!("Proxying {} {}{}", method, path, query);

    // Check for WebSocket upgrade
    if let Some(ws_upgrade) = ws {
        // This is a WebSocket upgrade request
        let upstream_url = format!(
            "{}{}{}",
            state.upstream_url.trim_end_matches('/'),
            path,
            query
        );

        // Convert HTTP URL to WebSocket URL
        let ws_url = upstream_url
            .replace("http://", "ws://")
            .replace("https://", "wss://");

        info!("WebSocket upgrade request -> {}", ws_url);

        return ws_upgrade.on_upgrade(move |socket| handle_websocket(socket, ws_url, state));
    }

    // Build upstream URL
    let upstream_url = format!(
        "{}{}{}",
        state.upstream_url.trim_end_matches('/'),
        path,
        query
    );

    // Read request body
    let body_bytes = match axum::body::to_bytes(req.into_body(), 10 * 1024 * 1024).await {
        Ok(bytes) => bytes,
        Err(e) => {
            error!("Failed to read request body: {}", e);
            return (StatusCode::BAD_REQUEST, "Failed to read request body").into_response();
        }
    };

    // Log incoming request
    if !body_bytes.is_empty() {
        log_message(&state, Direction::In, &body_bytes).await;
    }

    // Build upstream request
    let mut upstream_req = state.client.request(method.clone(), &upstream_url);

    // Forward relevant headers
    for (name, value) in headers.iter() {
        // Skip hop-by-hop headers
        let name_str = name.as_str().to_lowercase();
        if name_str == "host"
            || name_str == "connection"
            || name_str == "transfer-encoding"
            || name_str == "upgrade"
        {
            continue;
        }
        if let Ok(v) = value.to_str() {
            upstream_req = upstream_req.header(name.clone(), v);
        }
    }

    // Add body if present
    if !body_bytes.is_empty() {
        upstream_req = upstream_req.body(body_bytes.to_vec());
    }

    // Send to upstream
    let upstream_response = match upstream_req.send().await {
        Ok(resp) => resp,
        Err(e) => {
            error!("Upstream request failed: {}", e);
            return (
                StatusCode::BAD_GATEWAY,
                format!("Upstream request failed: {e}"),
            )
                .into_response();
        }
    };

    // Get response info
    let status = upstream_response.status();
    let resp_headers = upstream_response.headers().clone();
    let content_type = resp_headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    // Check if SSE response
    if content_type.contains("text/event-stream") {
        // Stream SSE response
        return stream_sse_response(state, upstream_response).await;
    }

    // Read response body
    let resp_body = match upstream_response.bytes().await {
        Ok(bytes) => bytes,
        Err(e) => {
            error!("Failed to read response body: {}", e);
            return (StatusCode::BAD_GATEWAY, "Failed to read response body").into_response();
        }
    };

    // Log outgoing response
    if !resp_body.is_empty() {
        log_message(&state, Direction::Out, &resp_body).await;
    }

    // Build response
    let mut response = Response::builder().status(StatusCode::from_u16(status.as_u16()).unwrap());

    // Forward response headers
    for (name, value) in resp_headers.iter() {
        let name_str = name.as_str().to_lowercase();
        if name_str == "transfer-encoding" || name_str == "connection" {
            continue;
        }
        response = response.header(name.clone(), value.clone());
    }

    response
        .body(Body::from(resp_body))
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

/// Stream SSE response while logging events
async fn stream_sse_response(state: HttpProxyState, response: reqwest::Response) -> Response {
    let status = response.status();
    let headers = response.headers().clone();

    // Create streaming body
    let stream = response.bytes_stream().map(move |result| {
        match result {
            Ok(chunk) => {
                // Log each SSE chunk
                let state_clone = state.clone();
                let chunk_clone = chunk.clone();
                tokio::spawn(async move {
                    log_message(&state_clone, Direction::Out, &chunk_clone).await;
                });
                Ok::<_, std::io::Error>(chunk)
            }
            Err(e) => {
                error!("SSE stream error: {}", e);
                Err(std::io::Error::new(std::io::ErrorKind::Other, e))
            }
        }
    });

    let body = Body::from_stream(stream);

    // Build response
    let mut response_builder =
        Response::builder().status(StatusCode::from_u16(status.as_u16()).unwrap());

    for (name, value) in headers.iter() {
        response_builder = response_builder.header(name.clone(), value.clone());
    }

    response_builder
        .body(body)
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

/// Handle WebSocket proxy - bidirectional forwarding with logging
async fn handle_websocket(client_socket: WebSocket, upstream_url: String, state: HttpProxyState) {
    info!(
        "Establishing WebSocket connection to upstream: {}",
        upstream_url
    );

    // Connect to upstream WebSocket server
    let upstream_ws = match connect_async(&upstream_url).await {
        Ok((ws_stream, _)) => ws_stream,
        Err(e) => {
            error!("Failed to connect to upstream WebSocket: {}", e);
            return;
        }
    };

    info!("WebSocket connection established to {}", upstream_url);

    // Split both connections into read/write halves
    let (mut client_write, mut client_read) = client_socket.split();
    let (mut upstream_write, mut upstream_read) = upstream_ws.split();

    let state_for_client = state.clone();
    let state_for_upstream = state.clone();

    // Task: Forward client messages to upstream (with logging)
    let client_to_upstream = tokio::spawn(async move {
        while let Some(msg_result) = client_read.next().await {
            match msg_result {
                Ok(msg) => {
                    // Log the message from client
                    if let Some(content) = ws_message_to_bytes(&msg) {
                        log_ws_message(&state_for_client, Direction::In, &content).await;
                    }

                    // Convert axum message to tungstenite message and forward
                    let tung_msg = axum_to_tungstenite(msg);
                    if let Some(m) = tung_msg {
                        if upstream_write.send(m).await.is_err() {
                            break;
                        }
                    }
                }
                Err(e) => {
                    debug!("Client WebSocket read error: {}", e);
                    break;
                }
            }
        }
        // Close upstream when client disconnects
        let _ = upstream_write.close().await;
    });

    // Task: Forward upstream messages to client (with logging)
    let upstream_to_client = tokio::spawn(async move {
        while let Some(msg_result) = upstream_read.next().await {
            match msg_result {
                Ok(msg) => {
                    // Log the message from upstream
                    if let Some(content) = tungstenite_message_to_bytes(&msg) {
                        log_ws_message(&state_for_upstream, Direction::Out, &content).await;
                    }

                    // Convert tungstenite message to axum message and forward
                    let axum_msg = tungstenite_to_axum(msg);
                    if let Some(m) = axum_msg {
                        if client_write.send(m).await.is_err() {
                            break;
                        }
                    }
                }
                Err(e) => {
                    debug!("Upstream WebSocket read error: {}", e);
                    break;
                }
            }
        }
        // Close client when upstream disconnects
        let _ = client_write.close().await;
    });

    // Wait for either direction to finish
    tokio::select! {
        _ = client_to_upstream => {
            debug!("Client to upstream task finished");
        }
        _ = upstream_to_client => {
            debug!("Upstream to client task finished");
        }
    }

    info!("WebSocket proxy session ended");
}

/// Convert axum WebSocket message to tungstenite message
fn axum_to_tungstenite(msg: AxumWsMessage) -> Option<TungsteniteMessage> {
    match msg {
        AxumWsMessage::Text(text) => Some(TungsteniteMessage::Text(text.to_string())),
        AxumWsMessage::Binary(data) => Some(TungsteniteMessage::Binary(data.to_vec())),
        AxumWsMessage::Ping(data) => Some(TungsteniteMessage::Ping(data.to_vec())),
        AxumWsMessage::Pong(data) => Some(TungsteniteMessage::Pong(data.to_vec())),
        AxumWsMessage::Close(_) => Some(TungsteniteMessage::Close(None)),
    }
}

/// Convert tungstenite message to axum WebSocket message
fn tungstenite_to_axum(msg: TungsteniteMessage) -> Option<AxumWsMessage> {
    match msg {
        TungsteniteMessage::Text(text) => Some(AxumWsMessage::Text(text)),
        TungsteniteMessage::Binary(data) => Some(AxumWsMessage::Binary(data)),
        TungsteniteMessage::Ping(data) => Some(AxumWsMessage::Ping(data)),
        TungsteniteMessage::Pong(data) => Some(AxumWsMessage::Pong(data)),
        TungsteniteMessage::Close(_) => Some(AxumWsMessage::Close(None)),
        TungsteniteMessage::Frame(_) => None, // Raw frames not supported
    }
}

/// Extract bytes from axum WebSocket message for logging
fn ws_message_to_bytes(msg: &AxumWsMessage) -> Option<Bytes> {
    match msg {
        AxumWsMessage::Text(text) => Some(Bytes::from(text.to_string())),
        AxumWsMessage::Binary(data) => Some(Bytes::copy_from_slice(data)),
        _ => None, // Don't log ping/pong/close
    }
}

/// Extract bytes from tungstenite WebSocket message for logging
fn tungstenite_message_to_bytes(msg: &TungsteniteMessage) -> Option<Bytes> {
    match msg {
        TungsteniteMessage::Text(text) => Some(Bytes::from(text.clone())),
        TungsteniteMessage::Binary(data) => Some(Bytes::from(data.clone())),
        _ => None, // Don't log ping/pong/close/frame
    }
}

/// Log a WebSocket message to the event sink
async fn log_ws_message(state: &HttpProxyState, direction: Direction, body: &Bytes) {
    let id = generate_message_id();

    // Try to parse as JSON
    let content = String::from_utf8_lossy(body);

    // Try to extract method from JSON-RPC
    let (method, message_type) =
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) {
            let method = json
                .get("method")
                .and_then(|m| m.as_str())
                .map(String::from);
            (method, MessageType::JsonRpc)
        } else {
            (None, MessageType::Raw)
        };

    let entry = LogEntry {
        id,
        session_id: state.session.id.clone(),
        timestamp: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_micros() as u64,
        direction,
        content: content.to_string(),
        method,
        duration_micros: None,
        message_type,
        token_count: TC::estimate_tokens(&content),
        server_name: Some(state.server_name.clone()),
    };

    if let Err(e) = state.event_sink.emit_log(&entry).await {
        warn!("Failed to emit WebSocket log: {}", e);
    }
}

/// Log a message to the event sink
async fn log_message(state: &HttpProxyState, direction: Direction, body: &Bytes) {
    let id = generate_message_id();

    // Try to parse as JSON
    let content = String::from_utf8_lossy(body);

    // Try to extract method from JSON-RPC
    let (method, message_type) =
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) {
            let method = json
                .get("method")
                .and_then(|m| m.as_str())
                .map(String::from);
            (method, MessageType::JsonRpc)
        } else {
            (None, MessageType::Raw)
        };

    let entry = LogEntry {
        id,
        session_id: state.session.id.clone(),
        timestamp: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_micros() as u64,
        direction,
        content: content.to_string(),
        method,
        duration_micros: None,
        message_type,
        token_count: TC::estimate_tokens(&content),
        server_name: Some(state.server_name.clone()),
    };

    if let Err(e) = state.event_sink.emit_log(&entry).await {
        warn!("Failed to emit log: {}", e);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_message_id_unique() {
        let id1 = generate_message_id();
        let id2 = generate_message_id();
        let id3 = generate_message_id();

        assert_ne!(id1, id2);
        assert_ne!(id2, id3);
        assert_ne!(id1, id3);
    }

    #[test]
    fn test_generate_message_id_format() {
        let id = generate_message_id();
        assert!(id.starts_with("http-"));
    }

    #[test]
    fn test_tungstenite_message_to_bytes_text() {
        let msg = TungsteniteMessage::Text("hello".to_string());
        let bytes = tungstenite_message_to_bytes(&msg);
        assert!(bytes.is_some());
        assert_eq!(bytes.unwrap().as_ref(), b"hello");
    }

    #[test]
    fn test_tungstenite_message_to_bytes_binary() {
        let msg = TungsteniteMessage::Binary(vec![1, 2, 3, 4]);
        let bytes = tungstenite_message_to_bytes(&msg);
        assert!(bytes.is_some());
        assert_eq!(bytes.unwrap().as_ref(), &[1, 2, 3, 4]);
    }

    #[test]
    fn test_tungstenite_message_to_bytes_ping() {
        let msg = TungsteniteMessage::Ping(vec![]);
        let bytes = tungstenite_message_to_bytes(&msg);
        assert!(bytes.is_none());
    }

    #[test]
    fn test_tungstenite_message_to_bytes_pong() {
        let msg = TungsteniteMessage::Pong(vec![]);
        let bytes = tungstenite_message_to_bytes(&msg);
        assert!(bytes.is_none());
    }

    #[test]
    fn test_tungstenite_message_to_bytes_close() {
        let msg = TungsteniteMessage::Close(None);
        let bytes = tungstenite_message_to_bytes(&msg);
        assert!(bytes.is_none());
    }

    #[test]
    fn test_tungstenite_to_axum_text() {
        let msg = TungsteniteMessage::Text("test".to_string());
        let axum_msg = tungstenite_to_axum(msg);
        assert!(axum_msg.is_some());
        match axum_msg.unwrap() {
            AxumWsMessage::Text(t) => assert_eq!(t.to_string(), "test"),
            _ => panic!("Expected Text message"),
        }
    }

    #[test]
    fn test_tungstenite_to_axum_binary() {
        let msg = TungsteniteMessage::Binary(vec![1, 2, 3]);
        let axum_msg = tungstenite_to_axum(msg);
        assert!(axum_msg.is_some());
        match axum_msg.unwrap() {
            AxumWsMessage::Binary(b) => assert_eq!(&b[..], &[1, 2, 3]),
            _ => panic!("Expected Binary message"),
        }
    }

    #[test]
    fn test_tungstenite_to_axum_ping() {
        let msg = TungsteniteMessage::Ping(vec![1, 2]);
        let axum_msg = tungstenite_to_axum(msg);
        assert!(axum_msg.is_some());
        match axum_msg.unwrap() {
            AxumWsMessage::Ping(p) => assert_eq!(&p[..], &[1, 2]),
            _ => panic!("Expected Ping message"),
        }
    }

    #[test]
    fn test_tungstenite_to_axum_pong() {
        let msg = TungsteniteMessage::Pong(vec![3, 4]);
        let axum_msg = tungstenite_to_axum(msg);
        assert!(axum_msg.is_some());
        match axum_msg.unwrap() {
            AxumWsMessage::Pong(p) => assert_eq!(&p[..], &[3, 4]),
            _ => panic!("Expected Pong message"),
        }
    }

    #[test]
    fn test_tungstenite_to_axum_close() {
        let msg = TungsteniteMessage::Close(None);
        let axum_msg = tungstenite_to_axum(msg);
        assert!(axum_msg.is_some());
        assert!(matches!(axum_msg.unwrap(), AxumWsMessage::Close(_)));
    }

    #[test]
    fn test_tungstenite_to_axum_frame() {
        // Frame messages are not supported
        let msg = TungsteniteMessage::Frame(
            tokio_tungstenite::tungstenite::protocol::frame::Frame::ping(vec![]),
        );
        let axum_msg = tungstenite_to_axum(msg);
        assert!(axum_msg.is_none());
    }

    #[test]
    fn test_http_event_sink_clone() {
        let sink = HttpEventSink::NoOp(NoOpEventSink);
        let _cloned = sink.clone();
    }

    #[test]
    fn test_http_proxy_state_clone() {
        let state = HttpProxyState {
            upstream_url: "http://localhost:8080".to_string(),
            session: create_session_id(Some("test")),
            server_name: "test-server".to_string(),
            client: Client::new(),
            event_sink: HttpEventSink::NoOp(NoOpEventSink),
            inject_tx: Arc::new(Mutex::new(None)),
        };
        let _cloned = state.clone();
    }
}
