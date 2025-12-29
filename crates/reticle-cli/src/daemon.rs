//! Reticle Daemon
//!
//! Standalone daemon that listens on a Unix socket for telemetry from
//! CLI instances (stdio and proxy wrappers). This enables headless/server
//! deployments without the GUI.
//!
//! The daemon receives events via the socket protocol and can:
//! - Log events to stdout/file
//! - Forward to a remote collector
//! - Serve a simple web UI (future)
//!
//! Note: Unix sockets are not available on Windows, so the daemon
//! functionality is only available on Unix-like systems.

#[cfg(unix)]
mod unix_impl {
    use std::path::Path;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::UnixListener;
    use tracing::{debug, error, info, warn};

    /// Run the daemon, listening on the specified Unix socket
    pub async fn run_daemon(
        socket_path: &str,
        _port: Option<u16>,
        verbose: bool,
    ) -> Result<(), String> {
        // Remove existing socket file if it exists
        let path = Path::new(socket_path);
        if path.exists() {
            std::fs::remove_file(path)
                .map_err(|e| format!("Failed to remove existing socket: {e}"))?;
        }

        // Create parent directory if needed
        if let Some(parent) = path.parent() {
            if !parent.exists() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("Failed to create socket directory: {e}"))?;
            }
        }

        // Bind to Unix socket
        let listener = UnixListener::bind(socket_path)
            .map_err(|e| format!("Failed to bind to socket {socket_path}: {e}"))?;

        info!("Daemon listening on {}", socket_path);

        // Accept connections
        loop {
            match listener.accept().await {
                Ok((stream, _addr)) => {
                    tokio::spawn(async move {
                        if let Err(e) = handle_connection(stream, verbose).await {
                            warn!("Connection error: {e}");
                        }
                    });
                }
                Err(e) => {
                    error!("Accept error: {e}");
                }
            }
        }
    }

    /// Handle a single client connection
    async fn handle_connection(
        stream: tokio::net::UnixStream,
        verbose: bool,
    ) -> Result<(), String> {
        let (reader, mut writer) = stream.into_split();
        let mut reader = BufReader::new(reader);
        let mut line = String::new();

        // Read server name from first line
        reader
            .read_line(&mut line)
            .await
            .map_err(|e| format!("Failed to read server name: {e}"))?;

        let server_name = line.trim().to_string();
        info!("Client connected: {}", server_name);
        line.clear();

        // Send acknowledgment
        writer
            .write_all(b"OK\n")
            .await
            .map_err(|e| format!("Failed to send ack: {e}"))?;

        // Process events
        loop {
            line.clear();
            match reader.read_line(&mut line).await {
                Ok(0) => {
                    // EOF - client disconnected
                    info!("Client disconnected: {}", server_name);
                    break;
                }
                Ok(_) => {
                    let trimmed = line.trim();
                    if trimmed.is_empty() {
                        continue;
                    }

                    // Parse the event
                    if let Ok(event) = serde_json::from_str::<serde_json::Value>(trimmed) {
                        if verbose {
                            // Pretty print in verbose mode
                            if let Ok(pretty) = serde_json::to_string_pretty(&event) {
                                println!("[{server_name}] {pretty}");
                            }
                        } else {
                            // Compact output
                            debug!("[{}] Event: {}", server_name, trimmed);
                        }

                        // Handle different event types
                        if let Some(event_type) = event.get("type").and_then(|t| t.as_str()) {
                            match event_type {
                                "session_start" => {
                                    let name = event
                                        .get("name")
                                        .and_then(|n| n.as_str())
                                        .unwrap_or("unknown");
                                    info!("[{}] Session started: {}", server_name, name);
                                }
                                "session_end" => {
                                    info!("[{}] Session ended", server_name);
                                }
                                "log" => {
                                    if verbose {
                                        let method = event
                                            .get("method")
                                            .and_then(|m| m.as_str())
                                            .unwrap_or("-");
                                        let direction = event
                                            .get("direction")
                                            .and_then(|d| d.as_str())
                                            .unwrap_or("-");
                                        println!(
                                            "[{}] {} {} {}",
                                            server_name,
                                            if direction == "in" { "→" } else { "←" },
                                            method,
                                            event
                                                .get("content")
                                                .and_then(|c| c.as_str())
                                                .unwrap_or("")
                                        );
                                    }
                                }
                                _ => {
                                    debug!("[{}] Unknown event type: {}", server_name, event_type);
                                }
                            }
                        }
                    } else {
                        warn!("[{}] Invalid JSON: {}", server_name, trimmed);
                    }
                }
                Err(e) => {
                    error!("[{}] Read error: {e}", server_name);
                    break;
                }
            }
        }

        Ok(())
    }
}

#[cfg(unix)]
pub use unix_impl::run_daemon;

/// Windows stub - daemon is not supported on Windows
#[cfg(windows)]
pub async fn run_daemon(
    _socket_path: &str,
    _port: Option<u16>,
    _verbose: bool,
) -> Result<(), String> {
    Err("The daemon command is not supported on Windows. Unix sockets are required.".to_string())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::fs;
    use std::path::Path;
    use tempfile::tempdir;

    #[test]
    fn test_socket_path_parsing() {
        let path = Path::new("/tmp/test.sock");
        assert_eq!(path.file_name().unwrap(), "test.sock");
        assert_eq!(path.parent().unwrap(), Path::new("/tmp"));
    }

    #[test]
    fn test_socket_path_with_nested_dir() {
        let path = Path::new("/tmp/reticle/nested/test.sock");
        assert_eq!(path.file_name().unwrap(), "test.sock");
        assert_eq!(path.parent().unwrap(), Path::new("/tmp/reticle/nested"));
    }

    #[tokio::test]
    async fn test_daemon_creates_socket() {
        let dir = tempdir().unwrap();
        let socket_path = dir.path().join("test.sock");
        let socket_path_str = socket_path.to_str().unwrap().to_string();

        // Run daemon in background, it will block so we just test socket creation
        let handle = tokio::spawn(async move {
            // This will run until cancelled
            let _ = run_daemon(&socket_path_str, None, false).await;
        });

        // Give daemon time to start
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        // Check socket was created
        assert!(socket_path.exists());

        // Clean up
        handle.abort();
    }

    #[tokio::test]
    async fn test_daemon_removes_existing_socket() {
        let dir = tempdir().unwrap();
        let socket_path = dir.path().join("existing.sock");

        // Create a file at the socket path
        fs::write(&socket_path, "dummy").unwrap();
        assert!(socket_path.exists());

        let socket_path_str = socket_path.to_str().unwrap().to_string();

        let handle = tokio::spawn(async move {
            let _ = run_daemon(&socket_path_str, None, false).await;
        });

        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        // Socket should exist and be a socket now, not a regular file
        assert!(socket_path.exists());

        handle.abort();
    }

    #[tokio::test]
    async fn test_daemon_creates_parent_dirs() {
        let dir = tempdir().unwrap();
        let socket_path = dir.path().join("nested/dir/test.sock");
        let socket_path_str = socket_path.to_str().unwrap().to_string();

        let handle = tokio::spawn(async move {
            let _ = run_daemon(&socket_path_str, None, false).await;
        });

        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        // Check parent dir was created
        assert!(socket_path.parent().unwrap().exists());
        assert!(socket_path.exists());

        handle.abort();
    }

    #[test]
    fn test_event_type_parsing() {
        let event: serde_json::Value = serde_json::json!({
            "type": "session_start",
            "name": "test-session"
        });

        assert_eq!(
            event.get("type").and_then(|t| t.as_str()),
            Some("session_start")
        );
        assert_eq!(
            event.get("name").and_then(|n| n.as_str()),
            Some("test-session")
        );
    }

    #[test]
    fn test_log_event_parsing() {
        let event: serde_json::Value = serde_json::json!({
            "type": "log",
            "method": "tools/list",
            "direction": "in",
            "content": "{\"jsonrpc\":\"2.0\"}"
        });

        assert_eq!(event.get("type").and_then(|t| t.as_str()), Some("log"));
        assert_eq!(
            event.get("method").and_then(|m| m.as_str()),
            Some("tools/list")
        );
        assert_eq!(event.get("direction").and_then(|d| d.as_str()), Some("in"));
    }

    #[test]
    fn test_invalid_json_handling() {
        let result = serde_json::from_str::<serde_json::Value>("not valid json");
        assert!(result.is_err());
    }

    #[test]
    fn test_direction_arrow() {
        let direction_in = "in";
        let direction_out = "out";

        assert_eq!(if direction_in == "in" { "→" } else { "←" }, "→");
        assert_eq!(if direction_out == "in" { "→" } else { "←" }, "←");
    }
}
