use tauri::{AppHandle, State};
use tokio::process::Command;

use crate::commands::demo::load_demo_data;
use crate::core::{
    run_proxy, start_sse_proxy, start_streamable_proxy, start_websocket_proxy, TransportConfig,
};
use crate::error::AppError;
use crate::events::session_events::emit_session_start;
use crate::security::generate_secure_session_id;
use crate::state::AppState;

/// Detected transport type from URL analysis
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetectedTransport {
    /// WebSocket transport (ws:// or wss://)
    WebSocket,
    /// Streamable HTTP transport (http:// or https://, default for HTTP)
    Streamable,
    /// Legacy SSE transport (explicit fallback)
    Sse,
}

/// Detect transport type from a URL
///
/// URL scheme detection:
/// - `ws://` or `wss://` → WebSocket
/// - `http://` or `https://` → Streamable HTTP (the newer 2025-03-26 spec)
///
/// The legacy SSE transport is not auto-detected but can be explicitly requested.
fn detect_transport_from_url(url: &str) -> Result<DetectedTransport, String> {
    let url_lower = url.to_lowercase();

    if url_lower.starts_with("ws://") || url_lower.starts_with("wss://") {
        Ok(DetectedTransport::WebSocket)
    } else if url_lower.starts_with("http://") || url_lower.starts_with("https://") {
        // Default to Streamable HTTP (newer spec, 2025-03-26)
        // Legacy SSE is deprecated and requires explicit selection
        Ok(DetectedTransport::Streamable)
    } else {
        Err(format!(
            "Invalid URL scheme. Expected ws://, wss://, http://, or https://. Got: {}",
            url
        ))
    }
}

/// Convert detected transport to TransportConfig
fn create_transport_config(
    detected: DetectedTransport,
    server_url: String,
    proxy_port: u16,
) -> TransportConfig {
    match detected {
        DetectedTransport::WebSocket => TransportConfig::WebSocket {
            server_url,
            proxy_port,
        },
        DetectedTransport::Streamable => TransportConfig::Streamable {
            server_url,
            proxy_port,
        },
        DetectedTransport::Sse => TransportConfig::Http {
            server_url,
            proxy_port,
        },
    }
}

/// Legacy Tauri command to start the proxy/demo
///
/// DEPRECATED: Use `start_proxy_v2` instead, which supports all transport types
/// and session naming. This function delegates to `start_proxy_v2`.
#[tauri::command]
pub async fn start_proxy(
    command: String,
    args: Vec<String>,
    app_handle: AppHandle,
    state: State<'_, AppState>,
) -> std::result::Result<String, String> {
    // Delegate to start_proxy_v2 with stdio transport config
    let transport_config = TransportConfig::Stdio { command, args };
    start_proxy_v2(transport_config, None, app_handle, state).await
}

/// Tauri command to stop the proxy/demo
#[tauri::command]
pub async fn stop_proxy(state: State<'_, AppState>) -> std::result::Result<(), String> {
    let mut proxy_state = state.proxy.lock().await;
    proxy_state.stop();
    Ok(())
}

/// Generate a default session name based on transport config
fn default_session_name(config: &TransportConfig) -> String {
    match config {
        TransportConfig::Stdio { command, .. } => {
            if command.is_empty() || command == "demo" {
                "Demo Session".to_string()
            } else {
                // Use the command name as the session name
                std::path::Path::new(command)
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or(command)
                    .to_string()
            }
        }
        TransportConfig::Http { server_url, .. } => {
            format!("HTTP: {server_url}")
        }
        TransportConfig::Streamable { server_url, .. } => {
            format!("Streamable: {server_url}")
        }
        TransportConfig::WebSocket { server_url, .. } => {
            format!("WebSocket: {server_url}")
        }
    }
}

/// Tauri command to start proxy with transport configuration
///
/// Supports all transport types: stdio, HTTP/SSE, Streamable HTTP, and WebSocket.
///
/// # Arguments
/// * `transport_config` - Configuration for the transport type
/// * `session_name` - Optional human-readable name for the session. If not provided,
///                    a default name is generated based on the transport type.
#[tauri::command]
pub async fn start_proxy_v2(
    transport_config: TransportConfig,
    session_name: Option<String>,
    app_handle: AppHandle,
    state: State<'_, AppState>,
) -> std::result::Result<String, String> {
    // Lock the proxy state
    let mut proxy_state = state.proxy.lock().await;

    // Check if already running
    if proxy_state.is_running() {
        return Err(AppError::ProxyAlreadyRunning.to_string());
    }

    // Generate cryptographically secure session ID
    let session_id = generate_secure_session_id();

    // Generate session name (use provided or generate default)
    let name = session_name.unwrap_or_else(|| default_session_name(&transport_config));

    proxy_state.start(session_id.clone());

    // Check if this is demo mode
    if transport_config.is_demo() {
        // Spawn demo data loading task
        let app_handle_clone = app_handle.clone();
        let proxy_arc = state.proxy.clone();
        let config = state.config.clone();
        let recorder_clone = state.recorder.clone();

        tauri::async_runtime::spawn(async move {
            if let Err(e) =
                load_demo_data(app_handle_clone, proxy_arc, config, recorder_clone).await
            {
                eprintln!("Error loading demo data: {e}");
            }
        });

        return Ok("Demo mode started successfully".to_string());
    }

    // Emit session start event
    let session_event = crate::events::session_events::SessionStartEvent {
        id: session_id.clone(),
        name: name.clone(),
        started_at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_micros() as u64,
    };

    if let Err(e) = emit_session_start(&app_handle, session_event) {
        eprintln!("Failed to emit session start: {e}");
    }

    // Match on transport type and start appropriate proxy
    match transport_config {
        TransportConfig::Stdio { command, args } => {
            // Validate command against allowlist
            if !state.config.security.is_command_allowed(&command) {
                proxy_state.stop();
                return Err(format!(
                    "Command '{}' is not in the allowed commands list. Allowed: {:?}",
                    command, state.config.security.allowed_commands
                ));
            }

            eprintln!("[STDIO PROXY] Command: {command} {args:?}");

            // Get project root directory (parent of src-tauri)
            let cwd = std::env::current_dir()
                .map_err(|e| format!("Failed to get current directory: {e}"))?;

            // If we're in src-tauri, go up one level to project root
            let project_root = if cwd.ends_with("src-tauri") {
                cwd.parent().unwrap_or(&cwd).to_path_buf()
            } else {
                cwd
            };

            eprintln!("[STDIO PROXY] Working directory: {project_root:?}");

            let mut child = Command::new(&command)
                .args(&args)
                .current_dir(&project_root)
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .map_err(|e| format!("Failed to spawn child process: {e}"))?;

            eprintln!("[STDIO PROXY] Child process spawned successfully");

            // Take stdin handle for interaction support
            let child_stdin = child
                .stdin
                .take()
                .ok_or_else(|| "Failed to capture child stdin".to_string())?;

            // Update proxy state with stdin handle for interaction
            proxy_state.start_with_stdin(session_id.clone(), child_stdin);
            drop(proxy_state); // Release lock before spawning async task

            // Run stdio proxy in background
            let app_handle_clone = app_handle.clone();
            let session_id_clone = session_id.clone();
            let recorder_clone = state.recorder.clone();

            tauri::async_runtime::spawn(async move {
                match run_proxy(child, session_id_clone, app_handle_clone, recorder_clone).await {
                    Ok(_) => {
                        println!("Stdio proxy completed successfully");
                    }
                    Err(e) => {
                        eprintln!("Stdio proxy error: {e}");
                    }
                }
            });

            Ok(format!("Stdio proxy started: {name} ({session_id})"))
        }

        TransportConfig::Http {
            server_url,
            proxy_port,
        } => {
            eprintln!("[HTTP PROXY] Starting on port {proxy_port} -> {server_url}");

            // Start SSE proxy server (legacy transport)
            let recorder_clone = state.recorder.clone();
            match start_sse_proxy(
                server_url.clone(),
                proxy_port,
                session_id.clone(),
                app_handle.clone(),
                recorder_clone,
            )
            .await
            {
                Ok(_handle) => {
                    // Store HTTP proxy URL for interaction support
                    let proxy_url = format!("http://localhost:{proxy_port}");
                    proxy_state.start_with_http(session_id.clone(), proxy_url);
                    drop(proxy_state); // Release lock

                    Ok(format!(
                        "HTTP/SSE proxy started: {name} (port {proxy_port} -> {server_url})"
                    ))
                }
                Err(e) => {
                    eprintln!("[HTTP PROXY ERROR] Failed to start: {e}");
                    Err(e)
                }
            }
        }

        TransportConfig::Streamable {
            server_url,
            proxy_port,
        } => {
            eprintln!("[STREAMABLE PROXY] Starting on port {proxy_port} -> {server_url}");

            // Start Streamable HTTP proxy server (MCP 2025-03-26)
            let recorder_clone = state.recorder.clone();
            match start_streamable_proxy(
                server_url.clone(),
                proxy_port,
                session_id.clone(),
                app_handle.clone(),
                recorder_clone,
            )
            .await
            {
                Ok(_handle) => {
                    // Store HTTP proxy URL for interaction support
                    let proxy_url = format!("http://localhost:{proxy_port}");
                    proxy_state.start_with_http(session_id.clone(), proxy_url);
                    drop(proxy_state); // Release lock

                    Ok(format!(
                        "Streamable HTTP proxy started: {name} (port {proxy_port} -> {server_url})"
                    ))
                }
                Err(e) => {
                    eprintln!("[STREAMABLE PROXY ERROR] Failed to start: {e}");
                    Err(e)
                }
            }
        }

        TransportConfig::WebSocket {
            server_url,
            proxy_port,
        } => {
            eprintln!("[WEBSOCKET PROXY] Starting on port {proxy_port} -> {server_url}");

            // Start WebSocket proxy server
            let recorder_clone = state.recorder.clone();
            match start_websocket_proxy(
                server_url.clone(),
                proxy_port,
                session_id.clone(),
                app_handle.clone(),
                recorder_clone,
            )
            .await
            {
                Ok(_handle) => {
                    // Store WebSocket proxy URL for interaction support
                    let proxy_url = format!("ws://localhost:{proxy_port}/ws");
                    proxy_state.start_with_http(session_id.clone(), proxy_url);
                    drop(proxy_state); // Release lock

                    Ok(format!(
                        "WebSocket proxy started: {name} (port {proxy_port} -> {server_url})"
                    ))
                }
                Err(e) => {
                    eprintln!("[WEBSOCKET PROXY ERROR] Failed to start: {e}");
                    Err(e)
                }
            }
        }
    }
}

/// Tauri command to start a remote proxy with auto-detected transport
///
/// This is the simplified API for connecting to remote MCP servers.
/// The transport type is automatically detected from the URL scheme:
/// - `ws://` or `wss://` → WebSocket transport
/// - `http://` or `https://` → Streamable HTTP transport (MCP 2025-03-26)
///
/// # Arguments
/// * `server_url` - The URL of the MCP server (e.g., "http://localhost:8080" or "ws://localhost:9000")
/// * `proxy_port` - The local port to run the proxy on
/// * `session_name` - Optional human-readable name for the session
/// * `use_legacy_sse` - If true, use legacy SSE transport instead of Streamable HTTP (for http:// URLs only)
#[tauri::command]
pub async fn start_remote_proxy(
    server_url: String,
    proxy_port: u16,
    session_name: Option<String>,
    use_legacy_sse: Option<bool>,
    app_handle: AppHandle,
    state: State<'_, AppState>,
) -> std::result::Result<String, String> {
    // Detect transport from URL
    let mut detected = detect_transport_from_url(&server_url)?;

    // Override to legacy SSE if explicitly requested (only for HTTP URLs)
    if use_legacy_sse.unwrap_or(false) && detected == DetectedTransport::Streamable {
        detected = DetectedTransport::Sse;
    }

    // Create transport config
    let transport_config = create_transport_config(detected, server_url, proxy_port);

    // Delegate to start_proxy_v2
    start_proxy_v2(transport_config, session_name, app_handle, state).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_transport_websocket() {
        assert_eq!(
            detect_transport_from_url("ws://localhost:9000").unwrap(),
            DetectedTransport::WebSocket
        );
        assert_eq!(
            detect_transport_from_url("wss://example.com/mcp").unwrap(),
            DetectedTransport::WebSocket
        );
        assert_eq!(
            detect_transport_from_url("WS://EXAMPLE.COM").unwrap(),
            DetectedTransport::WebSocket
        );
    }

    #[test]
    fn test_detect_transport_http() {
        assert_eq!(
            detect_transport_from_url("http://localhost:8080").unwrap(),
            DetectedTransport::Streamable
        );
        assert_eq!(
            detect_transport_from_url("https://example.com/mcp").unwrap(),
            DetectedTransport::Streamable
        );
        assert_eq!(
            detect_transport_from_url("HTTP://EXAMPLE.COM").unwrap(),
            DetectedTransport::Streamable
        );
    }

    #[test]
    fn test_detect_transport_invalid() {
        assert!(detect_transport_from_url("ftp://example.com").is_err());
        assert!(detect_transport_from_url("localhost:8080").is_err());
        assert!(detect_transport_from_url("").is_err());
    }

    #[test]
    fn test_create_transport_config() {
        let config = create_transport_config(
            DetectedTransport::WebSocket,
            "ws://localhost:9000".to_string(),
            3001,
        );
        assert!(matches!(config, TransportConfig::WebSocket { .. }));

        let config = create_transport_config(
            DetectedTransport::Streamable,
            "http://localhost:8080".to_string(),
            3001,
        );
        assert!(matches!(config, TransportConfig::Streamable { .. }));

        let config = create_transport_config(
            DetectedTransport::Sse,
            "http://localhost:8080".to_string(),
            3001,
        );
        assert!(matches!(config, TransportConfig::Http { .. }));
    }
}
