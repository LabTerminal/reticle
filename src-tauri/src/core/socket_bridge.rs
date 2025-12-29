//! Socket Bridge - Unix socket server for bidirectional CLI-GUI communication
//!
//! This module provides a Unix domain socket server that CLI instances can connect to
//! for bidirectional communication:
//! - CLI → GUI: Session events, log entries (telemetry)
//! - GUI → CLI: Inject commands (send messages to MCP server)
//!
//! Architecture:
//! - GUI creates socket at /tmp/reticle.sock on startup
//! - CLI connects and streams newline-delimited JSON events
//! - GUI can send inject_message commands back to CLI
//! - CLI injects messages into the wrapped MCP server's stdin
//!
//! Note: Unix sockets are not available on Windows, so this module
//! provides stub implementations on Windows.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;

/// Event types for socket communication (bidirectional)
#[derive(Debug, Clone, Serialize, Deserialize)]
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
        /// Target session ID
        session_id: String,
        /// The JSON-RPC message to inject
        message: String,
    },
}

/// Default socket path
pub const DEFAULT_SOCKET_PATH: &str = "/tmp/reticle.sock";

/// Get the socket path (can be overridden via env var)
pub fn get_socket_path() -> PathBuf {
    std::env::var("RETICLE_SOCKET")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(DEFAULT_SOCKET_PATH))
}

// ============================================================================
// Unix Implementation
// ============================================================================

#[cfg(unix)]
mod unix_impl {
    use super::*;
    use std::collections::HashMap;
    use tauri::{AppHandle, Emitter};
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::unix::OwnedWriteHalf;
    use tokio::net::{UnixListener, UnixStream};
    use tokio::sync::{broadcast, Mutex, RwLock};
    use tracing::{debug, error, info, warn};

    /// Active CLI session with its write handle for sending commands back
    #[allow(dead_code)] // Fields kept for debugging/logging context
    struct CliSession {
        session_id: String,
        server_name: String,
        writer: OwnedWriteHalf,
    }

    /// Shared state for tracking CLI sessions
    pub struct SocketBridgeState {
        /// Map of session_id → CLI session writer
        sessions: RwLock<HashMap<String, Arc<Mutex<CliSession>>>>,
    }

    impl SocketBridgeState {
        pub fn new() -> Self {
            Self {
                sessions: RwLock::new(HashMap::new()),
            }
        }

        /// Register a new CLI session
        async fn register_session(
            &self,
            session_id: String,
            server_name: String,
            writer: OwnedWriteHalf,
        ) {
            let session = CliSession {
                session_id: session_id.clone(),
                server_name,
                writer,
            };
            self.sessions
                .write()
                .await
                .insert(session_id, Arc::new(Mutex::new(session)));
        }

        /// Remove a CLI session
        async fn remove_session(&self, session_id: &str) {
            self.sessions.write().await.remove(session_id);
        }

        /// Get list of active CLI session IDs
        pub async fn get_active_sessions(&self) -> Vec<String> {
            self.sessions.read().await.keys().cloned().collect()
        }

        /// Check if a CLI session is active
        pub async fn has_session(&self, session_id: &str) -> bool {
            self.sessions.read().await.contains_key(session_id)
        }

        /// Send a message to a CLI session
        pub async fn send_to_session(&self, session_id: &str, message: &str) -> Result<(), String> {
            let sessions = self.sessions.read().await;
            let session = sessions
                .get(session_id)
                .ok_or_else(|| format!("Session {session_id} not found"))?
                .clone();
            drop(sessions);

            let mut session_guard = session.lock().await;

            // Create the inject message event
            let event = SocketEvent::InjectMessage {
                session_id: session_id.to_string(),
                message: message.to_string(),
            };

            let mut json = serde_json::to_string(&event)
                .map_err(|e| format!("Failed to serialize inject message: {e}"))?;
            json.push('\n');

            session_guard
                .writer
                .write_all(json.as_bytes())
                .await
                .map_err(|e| format!("Failed to send to CLI: {e}"))?;

            session_guard
                .writer
                .flush()
                .await
                .map_err(|e| format!("Failed to flush: {e}"))?;

            info!("Sent inject_message to session {}", session_id);
            Ok(())
        }
    }

    /// Global socket bridge state (will be stored in Tauri app state)
    pub static SOCKET_BRIDGE: once_cell::sync::Lazy<Arc<SocketBridgeState>> =
        once_cell::sync::Lazy::new(|| Arc::new(SocketBridgeState::new()));

    /// Get the socket bridge state
    pub fn get_socket_bridge() -> Arc<SocketBridgeState> {
        SOCKET_BRIDGE.clone()
    }

    /// Start the socket bridge server
    ///
    /// Creates a Unix socket that CLI instances can connect to for bidirectional communication.
    /// Returns a handle to the server task and a shutdown sender.
    pub async fn start_socket_bridge(
        app_handle: AppHandle,
    ) -> Result<(tokio::task::JoinHandle<()>, broadcast::Sender<()>), String> {
        let socket_path = get_socket_path();

        // Remove existing socket file if it exists
        if socket_path.exists() {
            std::fs::remove_file(&socket_path)
                .map_err(|e| format!("Failed to remove existing socket: {e}"))?;
        }

        info!("Starting socket bridge at {}", socket_path.display());

        let listener = UnixListener::bind(&socket_path)
            .map_err(|e| format!("Failed to bind socket at {}: {e}", socket_path.display()))?;

        // Set socket permissions to allow all users (for multi-user scenarios)
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::Permissions::from_mode(0o777);
            std::fs::set_permissions(&socket_path, perms)
                .map_err(|e| format!("Failed to set socket permissions: {e}"))?;
        }

        info!("Socket bridge listening at {}", socket_path.display());

        // Emit event to frontend that bridge is ready
        let _ = app_handle.emit(
            "socket-bridge-ready",
            serde_json::json!({ "path": socket_path.to_string_lossy() }),
        );

        let (shutdown_tx, _) = broadcast::channel::<()>(1);
        let shutdown_tx_clone = shutdown_tx.clone();

        let handle = tokio::spawn(async move {
            let mut shutdown_rx = shutdown_tx_clone.subscribe();

            loop {
                tokio::select! {
                    result = listener.accept() => {
                        match result {
                            Ok((stream, _addr)) => {
                                info!("CLI instance connected via socket");
                                let app = app_handle.clone();
                                let bridge = get_socket_bridge();
                                tokio::spawn(handle_connection(stream, app, bridge));
                            }
                            Err(e) => {
                                error!("Failed to accept connection: {}", e);
                            }
                        }
                    }
                    _ = shutdown_rx.recv() => {
                        info!("Socket bridge shutting down");
                        break;
                    }
                }
            }

            // Clean up socket file
            let _ = std::fs::remove_file(&socket_path);
        });

        Ok((handle, shutdown_tx))
    }

    /// Handle an individual CLI connection (bidirectional)
    async fn handle_connection(
        stream: UnixStream,
        app_handle: AppHandle,
        bridge: Arc<SocketBridgeState>,
    ) {
        // Split the stream for bidirectional communication
        let (read_half, write_half) = stream.into_split();
        let reader = BufReader::new(read_half);
        let mut lines = reader.lines();

        // We'll register the session once we receive session_started
        let mut current_session_id: Option<String> = None;
        let write_half = Arc::new(Mutex::new(Some(write_half)));

        while let Ok(Some(line)) = lines.next_line().await {
            if line.is_empty() {
                continue;
            }

            debug!("Received from CLI: {}", &line[..line.len().min(100)]);

            match serde_json::from_str::<SocketEvent>(&line) {
                Ok(event) => {
                    // Handle session registration
                    if let SocketEvent::SessionStarted {
                        ref session_id,
                        ref server_name,
                        ..
                    } = event
                    {
                        // Take the write half and register the session
                        if let Some(writer) = write_half.lock().await.take() {
                            bridge
                                .register_session(session_id.clone(), server_name.clone(), writer)
                                .await;
                            current_session_id = Some(session_id.clone());
                            info!("Registered CLI session: {}", session_id);
                        }
                    }

                    // Handle session end
                    if let SocketEvent::SessionEnded { ref session_id } = event {
                        bridge.remove_session(session_id).await;
                        info!("Unregistered CLI session: {}", session_id);
                    }

                    if let Err(e) = handle_event(&app_handle, event).await {
                        warn!("Failed to handle event: {}", e);
                    }
                }
                Err(e) => {
                    warn!(
                        "Failed to parse event: {} - {}",
                        e,
                        &line[..line.len().min(100)]
                    );
                }
            }
        }

        // Clean up session on disconnect
        if let Some(session_id) = current_session_id {
            bridge.remove_session(&session_id).await;
            info!("CLI session {} disconnected", session_id);
        }

        info!("CLI instance disconnected");
    }

    /// Handle a parsed event and emit to Tauri frontend
    async fn handle_event(app_handle: &AppHandle, event: SocketEvent) -> Result<(), String> {
        match event {
            SocketEvent::SessionStarted {
                session_id,
                session_name,
                server_name,
            } => {
                info!("CLI session started: {} ({})", session_name, session_id);
                debug!("Emitting session-start to frontend");

                let started_at = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_micros() as u64;

                app_handle
                    .emit(
                        "session-start",
                        serde_json::json!({
                            "id": session_id,
                            "started_at": started_at,
                            "session_name": session_name,
                            "server_name": server_name,
                            "from_cli": true,
                            "can_interact": true  // CLI sessions support interaction
                        }),
                    )
                    .map_err(|e| e.to_string())?;
            }
            SocketEvent::SessionEnded { session_id } => {
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
            SocketEvent::Log {
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
                    "CLI log event: {} {} {} tokens={} (id={})",
                    direction,
                    method.as_deref().unwrap_or("-"),
                    &content[..content.len().min(50)],
                    token_count,
                    id
                );

                app_handle
                    .emit(
                        "log-event",
                        serde_json::json!({
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
                        }),
                    )
                    .map_err(|e| e.to_string())?;
            }
            SocketEvent::InjectMessage { .. } => {
                // This is a GUI → CLI event, shouldn't be received here
                warn!("Received InjectMessage from CLI (unexpected)");
            }
        }

        Ok(())
    }
}

#[cfg(unix)]
#[allow(unused_imports)]
pub use unix_impl::{get_socket_bridge, start_socket_bridge, SocketBridgeState, SOCKET_BRIDGE};

// ============================================================================
// Windows Stub Implementation
// ============================================================================

#[cfg(windows)]
mod windows_impl {
    use super::*;
    use tauri::AppHandle;
    use tokio::sync::broadcast;
    use tracing::warn;

    /// Stub socket bridge state for Windows
    pub struct SocketBridgeState;

    impl SocketBridgeState {
        pub fn new() -> Self {
            Self
        }

        /// Get list of active CLI session IDs (always empty on Windows)
        pub async fn get_active_sessions(&self) -> Vec<String> {
            Vec::new()
        }

        /// Check if a CLI session is active (always false on Windows)
        pub async fn has_session(&self, _session_id: &str) -> bool {
            false
        }

        /// Send a message to a CLI session (not supported on Windows)
        pub async fn send_to_session(
            &self,
            _session_id: &str,
            _message: &str,
        ) -> Result<(), String> {
            Err("Socket bridge is not supported on Windows".to_string())
        }
    }

    /// Global socket bridge state stub
    pub static SOCKET_BRIDGE: once_cell::sync::Lazy<Arc<SocketBridgeState>> =
        once_cell::sync::Lazy::new(|| Arc::new(SocketBridgeState::new()));

    /// Get the socket bridge state
    pub fn get_socket_bridge() -> Arc<SocketBridgeState> {
        SOCKET_BRIDGE.clone()
    }

    /// Start the socket bridge server (no-op on Windows)
    pub async fn start_socket_bridge(
        _app_handle: AppHandle,
    ) -> Result<(tokio::task::JoinHandle<()>, broadcast::Sender<()>), String> {
        warn!("Socket bridge is not supported on Windows - CLI integration will not be available");

        let (shutdown_tx, _) = broadcast::channel::<()>(1);
        let handle = tokio::spawn(async {
            // No-op task that just waits forever
            std::future::pending::<()>().await;
        });

        Ok((handle, shutdown_tx))
    }
}

#[cfg(windows)]
pub use windows_impl::{get_socket_bridge, start_socket_bridge, SocketBridgeState, SOCKET_BRIDGE};
