//! Event Sink Trait
//!
//! This module provides the EventSink trait for decoupling event emission
//! from GUI frameworks. Implementations can emit events to Tauri, write
//! to stdout (CLI), or any other sink.

use async_trait::async_trait;
use serde::Serialize;

use crate::protocol::LogEntry;
use crate::session_recorder::RecordedSession;

/// Event sink for emitting events to listeners
///
/// This trait abstracts event emission so proxy logic can work with
/// different frontends (Tauri GUI, CLI, tests, etc.)
#[async_trait]
pub trait EventSink: Send + Sync {
    /// Emit a log event (new message intercepted)
    async fn emit_log(&self, entry: &LogEntry) -> Result<(), String>;

    /// Emit a session started event
    async fn emit_session_started(
        &self,
        session_id: &str,
        session_name: &str,
    ) -> Result<(), String>;

    /// Emit a session ended event
    async fn emit_session_ended(&self, session_id: &str) -> Result<(), String>;

    /// Emit a recording started event
    async fn emit_recording_started(&self, session_id: &str) -> Result<(), String>;

    /// Emit a recording stopped event
    async fn emit_recording_stopped(&self, session: &RecordedSession) -> Result<(), String>;

    /// Emit a generic event with custom payload
    async fn emit_custom<T: Serialize + Send + Sync>(
        &self,
        event_name: &str,
        payload: &T,
    ) -> Result<(), String>;
}

/// No-op event sink for testing or CLI mode without event emission
#[derive(Default, Clone)]
pub struct NoOpEventSink;

#[async_trait]
impl EventSink for NoOpEventSink {
    async fn emit_log(&self, _entry: &LogEntry) -> Result<(), String> {
        Ok(())
    }

    async fn emit_session_started(
        &self,
        _session_id: &str,
        _session_name: &str,
    ) -> Result<(), String> {
        Ok(())
    }

    async fn emit_session_ended(&self, _session_id: &str) -> Result<(), String> {
        Ok(())
    }

    async fn emit_recording_started(&self, _session_id: &str) -> Result<(), String> {
        Ok(())
    }

    async fn emit_recording_stopped(&self, _session: &RecordedSession) -> Result<(), String> {
        Ok(())
    }

    async fn emit_custom<T: Serialize + Send + Sync>(
        &self,
        _event_name: &str,
        _payload: &T,
    ) -> Result<(), String> {
        Ok(())
    }
}

/// Stdout event sink for CLI mode - prints events to console
#[derive(Default, Clone)]
pub struct StdoutEventSink {
    /// Whether to print in JSON format
    pub json_output: bool,
}

impl StdoutEventSink {
    pub fn new(json_output: bool) -> Self {
        Self { json_output }
    }
}

#[async_trait]
impl EventSink for StdoutEventSink {
    async fn emit_log(&self, entry: &LogEntry) -> Result<(), String> {
        // Always write to stderr to avoid polluting the proxied stdout stream
        if self.json_output {
            eprintln!("{}", serde_json::to_string(entry).unwrap_or_default());
        } else {
            let direction = match entry.direction {
                crate::protocol::Direction::In => "→",
                crate::protocol::Direction::Out => "←",
            };
            let method = entry.method.as_deref().unwrap_or("-");
            eprintln!(
                "[{}] {} {}",
                format_timestamp(entry.timestamp),
                direction,
                method
            );
        }
        Ok(())
    }

    async fn emit_session_started(
        &self,
        session_id: &str,
        session_name: &str,
    ) -> Result<(), String> {
        if self.json_output {
            eprintln!(
                r#"{{"event":"session_started","session_id":"{session_id}","name":"{session_name}"}}"#
            );
        } else {
            eprintln!("Session started: {session_name} ({session_id})");
        }
        Ok(())
    }

    async fn emit_session_ended(&self, session_id: &str) -> Result<(), String> {
        if self.json_output {
            eprintln!(r#"{{"event":"session_ended","session_id":"{session_id}"}}"#);
        } else {
            eprintln!("Session ended: {session_id}");
        }
        Ok(())
    }

    async fn emit_recording_started(&self, session_id: &str) -> Result<(), String> {
        if self.json_output {
            eprintln!(r#"{{"event":"recording_started","session_id":"{session_id}"}}"#);
        } else {
            eprintln!("Recording started: {session_id}");
        }
        Ok(())
    }

    async fn emit_recording_stopped(&self, session: &RecordedSession) -> Result<(), String> {
        if self.json_output {
            eprintln!(
                r#"{{"event":"recording_stopped","session_id":"{}","message_count":{}}}"#,
                session.id,
                session.messages.len()
            );
        } else {
            eprintln!(
                "Recording stopped: {} ({} messages)",
                session.id,
                session.messages.len()
            );
        }
        Ok(())
    }

    async fn emit_custom<T: Serialize + Send + Sync>(
        &self,
        event_name: &str,
        payload: &T,
    ) -> Result<(), String> {
        if self.json_output {
            let payload_json = serde_json::to_string(payload).unwrap_or_default();
            eprintln!(r#"{{"event":"{event_name}","payload":{payload_json}}}"#);
        } else {
            eprintln!("[{event_name}] Custom event");
        }
        Ok(())
    }
}

fn format_timestamp(micros: u64) -> String {
    let millis = micros / 1000;
    let secs = millis / 1000;
    let mins = secs / 60;
    let hours = mins / 60;
    format!(
        "{:02}:{:02}:{:02}.{:03}",
        hours % 24,
        mins % 60,
        secs % 60,
        millis % 1000
    )
}

/// WebSocket event sink for sending events to Reticle GUI
#[cfg(feature = "websocket")]
pub mod websocket {
    use super::*;
    use futures_util::{SinkExt, StreamExt};
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::sync::Mutex;
    use tokio_tungstenite::{connect_async, tungstenite::Message};

    type WsSender = futures_util::stream::SplitSink<
        tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
        Message,
    >;

    /// WebSocket event sink that sends events to Reticle GUI
    /// Supports auto-reconnection if GUI starts after CLI
    pub struct WebSocketEventSink {
        sender: Arc<Mutex<Option<WsSender>>>,
        server_name: String,
        url: String,
        session_id: Arc<Mutex<Option<String>>>,
    }

    impl WebSocketEventSink {
        /// Create a new WebSocket event sink and connect to the GUI
        /// If connection fails initially, will retry in background
        pub async fn connect(url: &str, server_name: String) -> Result<Self, String> {
            let sink = Self {
                sender: Arc::new(Mutex::new(None)),
                server_name: server_name.clone(),
                url: url.to_string(),
                session_id: Arc::new(Mutex::new(None)),
            };

            // Try to connect immediately
            if let Err(e) = sink.try_connect().await {
                tracing::warn!("Initial connection failed: {}, will retry in background", e);
                // Start background reconnection task
                sink.start_reconnect_task();
            }

            Ok(sink)
        }

        /// Try to establish WebSocket connection
        async fn try_connect(&self) -> Result<(), String> {
            let (ws_stream, _) = connect_async(&self.url)
                .await
                .map_err(|e| format!("Failed to connect to Reticle GUI: {e}"))?;

            let (sender, mut receiver) = ws_stream.split();

            // Store the sender
            {
                let mut guard = self.sender.lock().await;
                *guard = Some(sender);
            }

            tracing::info!("Connected to Reticle GUI at {}", self.url);

            // If we have a session, re-emit session_started
            let session_id = self.session_id.lock().await.clone();
            if let Some(sid) = session_id {
                let msg = serde_json::json!({
                    "type": "session_started",
                    "session_id": sid,
                    "session_name": &self.server_name,
                    "server_name": Some(&self.server_name),
                });
                let _ = self.send_internal(msg).await;
            }

            // Spawn task to handle incoming messages
            let sender_clone = self.sender.clone();
            let url_clone = self.url.clone();
            let server_name_clone = self.server_name.clone();
            let session_id_clone = self.session_id.clone();

            eprintln!("[reticle] Spawning receiver task");

            tokio::spawn(async move {
                eprintln!("[reticle] Receiver task started, waiting for messages");
                while let Some(msg) = receiver.next().await {
                    match msg {
                        Ok(Message::Text(text)) => {
                            eprintln!("[reticle] Received from GUI: {}", text);
                        }
                        Ok(Message::Close(frame)) => {
                            eprintln!("[reticle] GUI closed connection: {:?}", frame);
                            break;
                        }
                        Ok(Message::Ping(_)) => {
                            eprintln!("[reticle] Received ping from GUI");
                        }
                        Ok(Message::Pong(_)) => {
                            eprintln!("[reticle] Received pong from GUI");
                        }
                        Ok(other) => {
                            eprintln!("[reticle] Received other message: {:?}", other);
                        }
                        Err(e) => {
                            eprintln!("[reticle] WebSocket error in receiver: {}", e);
                            break;
                        }
                    }
                }
                eprintln!("[reticle] Receiver loop ended");

                // Connection lost, clear sender and try to reconnect
                {
                    let mut guard = sender_clone.lock().await;
                    *guard = None;
                }
                tracing::info!("Connection to GUI lost, will attempt to reconnect...");

                // Reconnect loop
                loop {
                    tokio::time::sleep(Duration::from_secs(2)).await;

                    match connect_async(&url_clone).await {
                        Ok((ws_stream, _)) => {
                            let (new_sender, mut new_receiver) = ws_stream.split();
                            {
                                let mut guard = sender_clone.lock().await;
                                *guard = Some(new_sender);
                            }
                            tracing::info!("Reconnected to Reticle GUI");
                            eprintln!("[reticle] Reconnected to GUI");

                            // Re-emit session if we have one
                            if let Some(sid) = session_id_clone.lock().await.clone() {
                                let msg = serde_json::json!({
                                    "type": "session_started",
                                    "session_id": sid,
                                    "session_name": &server_name_clone,
                                    "server_name": Some(&server_name_clone),
                                });
                                if let Some(sender) = sender_clone.lock().await.as_mut() {
                                    let _ = sender.send(Message::Text(msg.to_string())).await;
                                }
                            }

                            // Continue receiving
                            while let Some(msg) = new_receiver.next().await {
                                match msg {
                                    Ok(Message::Close(_)) | Err(_) => break,
                                    _ => {}
                                }
                            }

                            // Lost again, clear and retry
                            {
                                let mut guard = sender_clone.lock().await;
                                *guard = None;
                            }
                        }
                        Err(_) => {
                            // Silently retry
                        }
                    }
                }
            });

            Ok(())
        }

        /// Start background reconnection task
        fn start_reconnect_task(&self) {
            let sender = self.sender.clone();
            let url = self.url.clone();
            let server_name = self.server_name.clone();
            let session_id = self.session_id.clone();

            tokio::spawn(async move {
                loop {
                    tokio::time::sleep(Duration::from_secs(2)).await;

                    // Check if already connected
                    if sender.lock().await.is_some() {
                        continue;
                    }

                    match connect_async(&url).await {
                        Ok((ws_stream, _)) => {
                            let (new_sender, mut receiver) = ws_stream.split();
                            {
                                let mut guard = sender.lock().await;
                                *guard = Some(new_sender);
                            }
                            tracing::info!("Connected to Reticle GUI at {}", url);
                            eprintln!("[reticle] Connected to GUI");

                            // Re-emit session if we have one
                            if let Some(sid) = session_id.lock().await.clone() {
                                let msg = serde_json::json!({
                                    "type": "session_started",
                                    "session_id": sid,
                                    "session_name": &server_name,
                                    "server_name": Some(&server_name),
                                });
                                if let Some(s) = sender.lock().await.as_mut() {
                                    let _ = s.send(Message::Text(msg.to_string())).await;
                                }
                            }

                            // Handle incoming messages
                            while let Some(msg) = receiver.next().await {
                                match msg {
                                    Ok(Message::Close(_)) | Err(_) => {
                                        let mut guard = sender.lock().await;
                                        *guard = None;
                                        break;
                                    }
                                    _ => {}
                                }
                            }
                        }
                        Err(_) => {
                            // Silently retry
                        }
                    }
                }
            });
        }

        /// Internal send that doesn't update session state
        async fn send_internal(&self, msg: serde_json::Value) -> Result<(), String> {
            let mut guard = self.sender.lock().await;
            if let Some(sender) = guard.as_mut() {
                match sender.send(Message::Text(msg.to_string())).await {
                    Ok(_) => {
                        tracing::trace!("Sent message to GUI");
                        Ok(())
                    }
                    Err(e) => {
                        tracing::warn!("Failed to send to GUI: {}, clearing sender", e);
                        // Clear the sender so reconnect can happen
                        *guard = None;
                        Err(format!("Failed to send to GUI: {e}"))
                    }
                }
            } else {
                tracing::trace!("No GUI connection, message dropped");
                Ok(()) // Don't fail, just drop the message
            }
        }

        /// Send a message to the GUI
        async fn send(&self, msg: serde_json::Value) -> Result<(), String> {
            let result = self.send_internal(msg.clone()).await;
            if result.is_err() {
                tracing::debug!("Send failed (GUI not connected), message queued for reconnect");
            }
            result
        }
    }

    #[async_trait]
    impl EventSink for WebSocketEventSink {
        async fn emit_log(&self, entry: &LogEntry) -> Result<(), String> {
            let direction = match entry.direction {
                crate::protocol::Direction::In => "in",
                crate::protocol::Direction::Out => "out",
            };

            eprintln!(
                "[reticle] emit_log: {} {} {} (id={})",
                direction,
                entry.method.as_deref().unwrap_or("-"),
                &entry.content[..entry.content.len().min(50)],
                entry.id
            );

            let is_connected = self.sender.lock().await.is_some();
            eprintln!(
                "[reticle] GUI connected: {} (entry {})",
                is_connected, entry.id
            );

            let result = self
                .send(serde_json::json!({
                    "type": "log",
                    "id": entry.id,
                    "session_id": entry.session_id,
                    "timestamp": entry.timestamp,
                    "direction": direction,
                    "content": entry.content,
                    "method": entry.method,
                    "server_name": Some(&self.server_name),
                }))
                .await;

            if let Err(ref e) = result {
                eprintln!("[reticle] emit_log FAILED for {}: {}", entry.id, e);
            } else {
                eprintln!("[reticle] emit_log SUCCESS for {}", entry.id);
            }

            result
        }

        async fn emit_session_started(
            &self,
            session_id: &str,
            session_name: &str,
        ) -> Result<(), String> {
            // Store session ID for re-emission on reconnect
            {
                let mut guard = self.session_id.lock().await;
                *guard = Some(session_id.to_string());
            }

            self.send(serde_json::json!({
                "type": "session_started",
                "session_id": session_id,
                "session_name": session_name,
                "server_name": Some(&self.server_name),
            }))
            .await
        }

        async fn emit_session_ended(&self, session_id: &str) -> Result<(), String> {
            self.send(serde_json::json!({
                "type": "session_ended",
                "session_id": session_id,
            }))
            .await
        }

        async fn emit_recording_started(&self, session_id: &str) -> Result<(), String> {
            self.send(serde_json::json!({
                "type": "recording_started",
                "session_id": session_id,
            }))
            .await
        }

        async fn emit_recording_stopped(&self, session: &RecordedSession) -> Result<(), String> {
            self.send(serde_json::json!({
                "type": "recording_stopped",
                "session_id": session.id,
                "message_count": session.messages.len(),
            }))
            .await
        }

        async fn emit_custom<T: Serialize + Send + Sync>(
            &self,
            event_name: &str,
            payload: &T,
        ) -> Result<(), String> {
            self.send(serde_json::json!({
                "type": "custom",
                "event_name": event_name,
                "payload": payload,
            }))
            .await
        }
    }
}

#[cfg(feature = "websocket")]
pub use websocket::WebSocketEventSink;

/// Unix socket event sink for sub-10ms CLI-to-GUI communication
///
/// Uses Unix domain sockets for minimal latency:
/// - No TCP handshake overhead
/// - No WebSocket framing overhead
/// - Direct memory-to-memory transfer
/// - Newline-delimited JSON for simple parsing
#[cfg(unix)]
pub mod unix_socket {
    use super::*;
    use std::path::PathBuf;
    use std::sync::Arc;
    use tokio::io::AsyncWriteExt;
    use tokio::net::UnixStream;
    use tokio::sync::Mutex;

    /// Default socket path
    pub const DEFAULT_SOCKET_PATH: &str = "/tmp/reticle.sock";

    /// Get the socket path (can be overridden via env var)
    pub fn get_socket_path() -> PathBuf {
        std::env::var("RETICLE_SOCKET")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from(DEFAULT_SOCKET_PATH))
    }

    /// Event types sent over the socket (newline-delimited JSON)
    ///
    /// This enum is used for BOTH directions:
    /// - CLI → GUI: SessionStarted, SessionEnded, Log
    /// - GUI → CLI: InjectMessage
    #[derive(Debug, Clone, Serialize, serde::Deserialize)]
    #[serde(tag = "type")]
    pub enum SocketEvent {
        // === CLI → GUI events ===
        #[serde(rename = "session_started")]
        SessionStarted {
            session_id: String,
            session_name: String,
            server_name: String,
        },
        #[serde(rename = "session_ended")]
        SessionEnded { session_id: String },
        #[serde(rename = "log")]
        Log {
            id: String,
            session_id: String,
            timestamp: u64,
            direction: String,
            content: String,
            method: Option<String>,
            server_name: String,
            /// Type of message content (jsonrpc, raw, stderr)
            message_type: String,
            /// Estimated token count for this message
            token_count: u64,
        },

        // === GUI → CLI events ===
        /// Inject a message into the MCP server's stdin
        #[serde(rename = "inject_message")]
        InjectMessage {
            /// Target session ID (to ensure we send to the right CLI instance)
            session_id: String,
            /// The JSON-RPC message to inject
            message: String,
        },
    }

    use tokio::io::{AsyncBufReadExt, BufReader};
    use tokio::net::unix::OwnedWriteHalf;
    use tokio::sync::mpsc;

    /// Receiver for inject commands from the GUI
    pub type InjectReceiver = mpsc::Receiver<String>;

    /// Unix socket event sink for ultra-low latency event delivery
    ///
    /// Supports bidirectional communication:
    /// - Outgoing: Session events, log entries (CLI → GUI)
    /// - Incoming: Inject commands (GUI → CLI)
    pub struct UnixSocketEventSink {
        /// Write half of the socket for sending events
        writer: Arc<Mutex<Option<OwnedWriteHalf>>>,
        server_name: String,
        socket_path: PathBuf,
        session_id: Arc<Mutex<Option<String>>>,
        /// Sender for inject commands received from GUI
        inject_tx: mpsc::Sender<String>,
    }

    impl UnixSocketEventSink {
        /// Create a new Unix socket event sink
        ///
        /// Returns the sink and a receiver for inject commands.
        /// The receiver should be used by the proxy to receive messages to inject.
        ///
        /// Attempts to connect immediately, retries in background if failed.
        /// Designed for "fail-open" operation - if Hub is unavailable,
        /// the sink silently drops events without affecting the proxy.
        pub async fn new(server_name: String) -> (Self, InjectReceiver) {
            let socket_path = get_socket_path();
            let (inject_tx, inject_rx) = mpsc::channel(100);

            let sink = Self {
                writer: Arc::new(Mutex::new(None)),
                server_name,
                socket_path: socket_path.clone(),
                session_id: Arc::new(Mutex::new(None)),
                inject_tx: inject_tx.clone(),
            };

            // Try to connect immediately (silent on failure)
            if let Ok(stream) = UnixStream::connect(&socket_path).await {
                sink.setup_connection(stream).await;
                tracing::debug!("Connected to Reticle Hub at {}", socket_path.display());
            } else {
                // Start background reconnection - no stderr noise
                tracing::debug!("Reticle Hub not available, will retry in background");
                sink.start_reconnect_task();
            }

            (sink, inject_rx)
        }

        /// Set the current session ID (used for filtering incoming inject commands)
        pub async fn set_session_id(&self, session_id: String) {
            *self.session_id.lock().await = Some(session_id);
        }

        /// Setup bidirectional connection
        async fn setup_connection(&self, stream: UnixStream) {
            let (read_half, write_half) = stream.into_split();

            // Store the write half for sending
            *self.writer.lock().await = Some(write_half);

            // Start reader task for incoming inject commands
            let inject_tx = self.inject_tx.clone();
            let session_id = self.session_id.clone();
            let writer = self.writer.clone();

            tokio::spawn(async move {
                let reader = BufReader::new(read_half);
                let mut lines = reader.lines();

                while let Ok(Some(line)) = lines.next_line().await {
                    if line.is_empty() {
                        continue;
                    }

                    // Parse the incoming event
                    if let Ok(SocketEvent::InjectMessage {
                        session_id: target_session,
                        message,
                    }) = serde_json::from_str::<SocketEvent>(&line)
                    {
                        // Check if this message is for our session
                        let our_session = session_id.lock().await.clone();
                        if our_session.as_ref() == Some(&target_session) {
                            tracing::debug!("Received inject command for our session");
                            if let Err(e) = inject_tx.send(message).await {
                                tracing::warn!("Failed to forward inject command: {}", e);
                            }
                        }
                    }
                }

                // Connection closed, clear the writer
                tracing::debug!("Socket read connection closed");
                *writer.lock().await = None;
            });
        }

        /// Start background task to reconnect if disconnected
        fn start_reconnect_task(&self) {
            let writer = self.writer.clone();
            let socket_path = self.socket_path.clone();
            let inject_tx = self.inject_tx.clone();
            let session_id = self.session_id.clone();

            tokio::spawn(async move {
                loop {
                    // Wait before retry (longer interval to reduce overhead)
                    tokio::time::sleep(std::time::Duration::from_secs(2)).await;

                    // Check if already connected
                    if writer.lock().await.is_some() {
                        continue;
                    }

                    // Try to connect (silent on failure)
                    if let Ok(stream) = UnixStream::connect(&socket_path).await {
                        let (read_half, write_half) = stream.into_split();
                        *writer.lock().await = Some(write_half);
                        tracing::debug!("Reconnected to Reticle Hub");

                        // Start reader task
                        let inject_tx = inject_tx.clone();
                        let session_id = session_id.clone();
                        let writer_for_close = writer.clone();

                        tokio::spawn(async move {
                            let reader = BufReader::new(read_half);
                            let mut lines = reader.lines();

                            while let Ok(Some(line)) = lines.next_line().await {
                                if line.is_empty() {
                                    continue;
                                }

                                if let Ok(SocketEvent::InjectMessage {
                                    session_id: target_session,
                                    message,
                                }) = serde_json::from_str::<SocketEvent>(&line)
                                {
                                    let our_session = session_id.lock().await.clone();
                                    if our_session.as_ref() == Some(&target_session) {
                                        let _ = inject_tx.send(message).await;
                                    }
                                }
                            }

                            *writer_for_close.lock().await = None;
                        });
                    }
                }
            });
        }

        /// Send an event over the socket
        ///
        /// Fail-open: returns Ok(()) even if not connected or write fails.
        /// Observability should never degrade agent performance.
        async fn send(&self, event: &SocketEvent) -> Result<(), String> {
            let mut guard = self.writer.lock().await;

            if let Some(writer) = guard.as_mut() {
                let mut json = serde_json::to_string(event)
                    .map_err(|e| format!("Failed to serialize event: {e}"))?;
                json.push('\n');

                tracing::trace!("Sending {} bytes to socket", json.len());

                match writer.write_all(json.as_bytes()).await {
                    Ok(_) => {
                        // Flush to ensure data is sent immediately
                        if let Err(e) = writer.flush().await {
                            tracing::debug!("Flush failed: {}, clearing connection", e);
                            *guard = None;
                        }
                    }
                    Err(e) => {
                        // Connection lost, clear writer for reconnect
                        tracing::debug!("Socket write failed: {}, will reconnect", e);
                        *guard = None;
                    }
                }
            }
            // Not connected or write failed - silently drop (fail-open)
            Ok(())
        }
    }

    #[async_trait]
    impl EventSink for UnixSocketEventSink {
        async fn emit_log(&self, entry: &LogEntry) -> Result<(), String> {
            let direction = match entry.direction {
                crate::protocol::Direction::In => "in",
                crate::protocol::Direction::Out => "out",
            };

            let message_type = match entry.message_type {
                crate::protocol::MessageType::JsonRpc => "jsonrpc",
                crate::protocol::MessageType::Raw => "raw",
                crate::protocol::MessageType::Stderr => "stderr",
            };

            let event = SocketEvent::Log {
                id: entry.id.clone(),
                session_id: entry.session_id.clone(),
                timestamp: entry.timestamp,
                direction: direction.to_string(),
                content: entry.content.clone(),
                method: entry.method.clone(),
                server_name: self.server_name.clone(),
                message_type: message_type.to_string(),
                token_count: entry.token_count,
            };

            self.send(&event).await
        }

        async fn emit_session_started(
            &self,
            session_id: &str,
            session_name: &str,
        ) -> Result<(), String> {
            let event = SocketEvent::SessionStarted {
                session_id: session_id.to_string(),
                session_name: session_name.to_string(),
                server_name: self.server_name.clone(),
            };

            self.send(&event).await
        }

        async fn emit_session_ended(&self, session_id: &str) -> Result<(), String> {
            let event = SocketEvent::SessionEnded {
                session_id: session_id.to_string(),
            };

            self.send(&event).await
        }

        async fn emit_recording_started(&self, _session_id: &str) -> Result<(), String> {
            // Not needed for CLI bridge
            Ok(())
        }

        async fn emit_recording_stopped(&self, _session: &RecordedSession) -> Result<(), String> {
            // Not needed for CLI bridge
            Ok(())
        }

        async fn emit_custom<T: Serialize + Send + Sync>(
            &self,
            _event_name: &str,
            _payload: &T,
        ) -> Result<(), String> {
            // Not needed for CLI bridge
            Ok(())
        }
    }
}

/// Windows stub module - Unix sockets are not available on Windows
#[cfg(windows)]
pub mod unix_socket {
    use super::*;
    use std::path::PathBuf;
    use tokio::sync::mpsc;

    /// Default socket path (not used on Windows)
    pub const DEFAULT_SOCKET_PATH: &str = "";

    /// Get the socket path (not used on Windows)
    pub fn get_socket_path() -> PathBuf {
        PathBuf::new()
    }

    /// Event types sent over the socket
    #[derive(Debug, Clone, Serialize, serde::Deserialize)]
    #[serde(tag = "type")]
    pub enum SocketEvent {
        #[serde(rename = "session_started")]
        SessionStarted {
            session_id: String,
            session_name: String,
            server_name: String,
        },
        #[serde(rename = "session_ended")]
        SessionEnded { session_id: String },
        #[serde(rename = "log")]
        Log {
            id: String,
            session_id: String,
            timestamp: u64,
            direction: String,
            content: String,
            method: Option<String>,
            server_name: String,
            message_type: String,
            token_count: u64,
        },
        #[serde(rename = "inject_message")]
        InjectMessage { session_id: String, message: String },
    }

    /// Receiver for inject commands (stub on Windows)
    pub type InjectReceiver = mpsc::Receiver<String>;

    /// Unix socket event sink stub for Windows
    pub struct UnixSocketEventSink {
        _server_name: String,
        _inject_tx: mpsc::Sender<String>,
    }

    impl UnixSocketEventSink {
        /// Create a new stub event sink (no-op on Windows)
        pub async fn new(server_name: String) -> (Self, InjectReceiver) {
            let (inject_tx, inject_rx) = mpsc::channel(1);
            (
                Self {
                    _server_name: server_name,
                    _inject_tx: inject_tx,
                },
                inject_rx,
            )
        }

        /// Set session ID (no-op on Windows)
        pub async fn set_session_id(&self, _session_id: String) {}
    }

    #[async_trait]
    impl EventSink for UnixSocketEventSink {
        async fn emit_log(&self, _entry: &LogEntry) -> Result<(), String> {
            Ok(())
        }

        async fn emit_session_started(
            &self,
            _session_id: &str,
            _session_name: &str,
        ) -> Result<(), String> {
            Ok(())
        }

        async fn emit_session_ended(&self, _session_id: &str) -> Result<(), String> {
            Ok(())
        }

        async fn emit_recording_started(&self, _session_id: &str) -> Result<(), String> {
            Ok(())
        }

        async fn emit_recording_stopped(&self, _session: &RecordedSession) -> Result<(), String> {
            Ok(())
        }

        async fn emit_custom<T: Serialize + Send + Sync>(
            &self,
            _event_name: &str,
            _payload: &T,
        ) -> Result<(), String> {
            Ok(())
        }
    }
}

pub use unix_socket::{
    get_socket_path, InjectReceiver, SocketEvent, UnixSocketEventSink, DEFAULT_SOCKET_PATH,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_noop_sink() {
        let sink = NoOpEventSink;
        assert!(sink
            .emit_session_started("test", "Test Session")
            .await
            .is_ok());
        assert!(sink.emit_session_ended("test").await.is_ok());
    }

    #[test]
    fn test_format_timestamp() {
        assert_eq!(format_timestamp(0), "00:00:00.000");
        assert_eq!(format_timestamp(1_000_000), "00:00:01.000");
        assert_eq!(format_timestamp(3_661_500_000), "01:01:01.500");
    }
}
