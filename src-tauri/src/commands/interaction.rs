//! Interaction commands for sending MCP requests
//!
//! This module handles bidirectional communication with MCP servers,
//! allowing users to send custom JSON-RPC requests and receive responses.

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};
use tauri::{AppHandle, Emitter, State};

use crate::core::protocol::{Direction, LogEntry};
use crate::core::session_recorder::MessageDirection;
use crate::state::AppState;

/// Counter for generating unique request IDs
static REQUEST_COUNTER: AtomicU64 = AtomicU64::new(1);

/// Request to send to the MCP server
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendRequestParams {
    /// The JSON-RPC method to call
    pub method: String,
    /// Optional parameters for the method
    pub params: Option<serde_json::Value>,
    /// Optional custom ID (if not provided, one will be generated)
    pub id: Option<serde_json::Value>,
}

/// Response from send_request command
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendRequestResult {
    /// The ID of the sent request (for correlation)
    pub request_id: serde_json::Value,
    /// The full JSON-RPC request that was sent
    pub request: serde_json::Value,
}

/// Generate a unique request ID
fn generate_request_id() -> serde_json::Value {
    let counter = REQUEST_COUNTER.fetch_add(1, Ordering::SeqCst);
    serde_json::json!(format!("req-{}", counter))
}

/// Send a JSON-RPC request to the MCP server
///
/// This command sends a request via the appropriate transport (stdio or HTTP)
/// and returns immediately. The response will arrive via the normal log-event stream.
#[tauri::command]
pub async fn send_request(
    params: SendRequestParams,
    app_handle: AppHandle,
    state: State<'_, AppState>,
) -> Result<SendRequestResult, String> {
    let proxy_state = state.proxy.lock().await;

    if !proxy_state.is_running() {
        return Err("Proxy is not running".to_string());
    }

    // Generate or use provided ID
    let request_id = params.id.unwrap_or_else(generate_request_id);

    // Build the JSON-RPC request
    let request = if let Some(p) = params.params {
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": request_id,
            "method": params.method,
            "params": p
        })
    } else {
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": request_id,
            "method": params.method
        })
    };

    let request_str =
        serde_json::to_string(&request).map_err(|e| format!("Failed to serialize request: {e}"))?;

    if !proxy_state.can_send() {
        return Err("Cannot send messages - no transport available".to_string());
    }

    // Send via stdio if available
    if proxy_state.is_stdio() {
        proxy_state.send_message(&request_str).await?;

        // Log the sent message
        let session_id = proxy_state
            .session_id
            .clone()
            .unwrap_or_else(|| "unknown".to_string());
        let log_id = format!("sent-{}", REQUEST_COUNTER.load(Ordering::SeqCst));

        let entry = LogEntry::new(
            log_id,
            session_id.clone(),
            Direction::In, // Direction::In means we sent it TO the server
            request.clone(),
        );

        // Emit to frontend so user sees their sent request
        if let Err(e) = app_handle.emit("log-event", &entry) {
            eprintln!("[INTERACTION] Failed to emit sent request: {e}");
        }

        // Record if recording is active
        let recorder_lock = state.recorder.lock().await;
        if let Some(ref rec) = *recorder_lock {
            if let Err(e) = rec
                .record_message(request.clone(), MessageDirection::ToServer)
                .await
            {
                eprintln!("[INTERACTION] Failed to record sent message: {e}");
            }
        }
        drop(recorder_lock);

        Ok(SendRequestResult {
            request_id,
            request,
        })
    } else if proxy_state.is_http() {
        // Send via HTTP POST to the proxy's /message endpoint
        let proxy_url = proxy_state
            .get_http_proxy_url()
            .ok_or_else(|| "HTTP proxy URL not available".to_string())?
            .to_string();

        // Release the lock before making HTTP request
        drop(proxy_state);

        let client = reqwest::Client::new();
        let response = client
            .post(format!("{proxy_url}/message"))
            .json(&serde_json::json!({ "message": request }))
            .send()
            .await
            .map_err(|e| format!("Failed to send HTTP request: {e}"))?;

        if response.status().is_success() {
            // The proxy will emit log events, so we just return success
            Ok(SendRequestResult {
                request_id,
                request,
            })
        } else {
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            Err(format!("HTTP request failed: {error_text}"))
        }
    } else {
        Err("No transport available for sending messages".to_string())
    }
}

/// Send a raw JSON-RPC message (for advanced users)
///
/// This sends the message exactly as provided, without modification.
#[tauri::command]
pub async fn send_raw_message(
    message: String,
    app_handle: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    // Validate it's valid JSON
    let json: serde_json::Value =
        serde_json::from_str(&message).map_err(|e| format!("Invalid JSON: {e}"))?;

    let proxy_state = state.proxy.lock().await;

    if !proxy_state.is_running() {
        return Err("Proxy is not running".to_string());
    }

    if !proxy_state.can_send() {
        return Err("Cannot send messages - no transport available".to_string());
    }

    if proxy_state.is_stdio() {
        proxy_state.send_message(&message).await?;

        // Log the sent message
        let session_id = proxy_state
            .session_id
            .clone()
            .unwrap_or_else(|| "unknown".to_string());
        let log_id = format!("raw-{}", REQUEST_COUNTER.fetch_add(1, Ordering::SeqCst));

        let entry = LogEntry::new(log_id, session_id, Direction::In, json.clone());

        if let Err(e) = app_handle.emit("log-event", &entry) {
            eprintln!("[INTERACTION] Failed to emit raw message: {e}");
        }

        // Record if recording is active
        let recorder_lock = state.recorder.lock().await;
        if let Some(ref rec) = *recorder_lock {
            if let Err(e) = rec.record_message(json, MessageDirection::ToServer).await {
                eprintln!("[INTERACTION] Failed to record raw message: {e}");
            }
        }

        Ok(())
    } else if proxy_state.is_http() {
        // Send via HTTP POST
        let proxy_url = proxy_state
            .get_http_proxy_url()
            .ok_or_else(|| "HTTP proxy URL not available".to_string())?
            .to_string();

        drop(proxy_state);

        let client = reqwest::Client::new();
        let response = client
            .post(format!("{proxy_url}/message"))
            .json(&serde_json::json!({ "message": json }))
            .send()
            .await
            .map_err(|e| format!("Failed to send HTTP request: {e}"))?;

        if response.status().is_success() {
            Ok(())
        } else {
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            Err(format!("HTTP request failed: {error_text}"))
        }
    } else {
        Err("No transport available for sending messages".to_string())
    }
}

/// Check if interaction (sending) is available
///
/// Returns true if either:
/// - A GUI-managed proxy is running (stdio or HTTP)
/// - A CLI session is active via socket bridge
#[tauri::command]
pub async fn can_interact(state: State<'_, AppState>) -> Result<bool, String> {
    // Check GUI-managed proxy first
    let proxy_state = state.proxy.lock().await;
    if proxy_state.can_send() {
        return Ok(true);
    }
    drop(proxy_state);

    // Check for active CLI sessions via socket bridge
    let bridge = crate::core::socket_bridge::get_socket_bridge();
    let sessions = bridge.get_active_sessions().await;
    Ok(!sessions.is_empty())
}

/// Get list of active CLI sessions that support interaction
#[tauri::command]
pub async fn get_cli_sessions() -> Result<Vec<String>, String> {
    let bridge = crate::core::socket_bridge::get_socket_bridge();
    Ok(bridge.get_active_sessions().await)
}

/// Send a message to a CLI session
///
/// Injects a JSON-RPC message into the MCP server's stdin via the CLI wrapper.
#[tauri::command]
pub async fn send_to_cli_session(
    session_id: String,
    message: String,
    app_handle: AppHandle,
) -> Result<(), String> {
    // Validate JSON
    let json: serde_json::Value =
        serde_json::from_str(&message).map_err(|e| format!("Invalid JSON: {e}"))?;

    let bridge = crate::core::socket_bridge::get_socket_bridge();

    // Check session exists
    if !bridge.has_session(&session_id).await {
        return Err(format!("CLI session {session_id} not found"));
    }

    // Send the message
    bridge.send_to_session(&session_id, &message).await?;

    // Emit the sent message as a log event so user sees it in the UI
    let log_id = format!(
        "cli-inject-{}",
        REQUEST_COUNTER.fetch_add(1, Ordering::SeqCst)
    );
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_micros() as u64;

    let _ = app_handle.emit(
        "log-event",
        serde_json::json!({
            "id": log_id,
            "session_id": session_id,
            "timestamp": timestamp,
            "direction": "in",
            "content": message,
            "method": json.get("method").and_then(|m| m.as_str()),
            "message_type": "jsonrpc",
            "from_gui_inject": true
        }),
    );

    Ok(())
}

/// Get common MCP methods for quick access
#[tauri::command]
pub fn get_mcp_methods() -> Vec<McpMethodInfo> {
    vec![
        McpMethodInfo {
            method: "initialize".to_string(),
            description: "Initialize the MCP connection".to_string(),
            example_params: Some(serde_json::json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {
                    "roots": { "listChanged": true },
                    "sampling": {}
                },
                "clientInfo": {
                    "name": "reticle",
                    "version": "0.1.0"
                }
            })),
        },
        McpMethodInfo {
            method: "initialized".to_string(),
            description: "Notify server that client is initialized (notification, no response)"
                .to_string(),
            example_params: None,
        },
        McpMethodInfo {
            method: "tools/list".to_string(),
            description: "List available tools".to_string(),
            example_params: None,
        },
        McpMethodInfo {
            method: "tools/call".to_string(),
            description: "Call a tool with arguments".to_string(),
            example_params: Some(serde_json::json!({
                "name": "example_tool",
                "arguments": {}
            })),
        },
        McpMethodInfo {
            method: "resources/list".to_string(),
            description: "List available resources".to_string(),
            example_params: None,
        },
        McpMethodInfo {
            method: "resources/read".to_string(),
            description: "Read a resource".to_string(),
            example_params: Some(serde_json::json!({
                "uri": "file:///example.txt"
            })),
        },
        McpMethodInfo {
            method: "prompts/list".to_string(),
            description: "List available prompts".to_string(),
            example_params: None,
        },
        McpMethodInfo {
            method: "prompts/get".to_string(),
            description: "Get a prompt with arguments".to_string(),
            example_params: Some(serde_json::json!({
                "name": "example_prompt",
                "arguments": {}
            })),
        },
        McpMethodInfo {
            method: "ping".to_string(),
            description: "Ping the server (keep-alive)".to_string(),
            example_params: None,
        },
    ]
}

/// Information about an MCP method
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpMethodInfo {
    /// The method name
    pub method: String,
    /// Description of what the method does
    pub description: String,
    /// Example parameters (if any)
    pub example_params: Option<serde_json::Value>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_send_request_params_serialization() {
        let params = SendRequestParams {
            method: "tools/list".to_string(),
            params: Some(serde_json::json!({"key": "value"})),
            id: Some(serde_json::json!("req-123")),
        };

        let json = serde_json::to_string(&params).unwrap();
        assert!(json.contains("\"method\":\"tools/list\""));
        assert!(json.contains("\"key\":\"value\""));
        assert!(json.contains("\"id\":\"req-123\""));
    }

    #[test]
    fn test_send_request_params_deserialization() {
        let json = r#"{"method":"ping","params":null,"id":1}"#;
        let params: SendRequestParams = serde_json::from_str(json).unwrap();

        assert_eq!(params.method, "ping");
        assert!(params.params.is_none());
        assert_eq!(params.id, Some(serde_json::json!(1)));
    }

    #[test]
    fn test_send_request_params_minimal() {
        let json = r#"{"method":"initialize"}"#;
        let params: SendRequestParams = serde_json::from_str(json).unwrap();

        assert_eq!(params.method, "initialize");
        assert!(params.params.is_none());
        assert!(params.id.is_none());
    }

    #[test]
    fn test_send_request_result_serialization() {
        let result = SendRequestResult {
            request_id: serde_json::json!("req-1"),
            request: serde_json::json!({
                "jsonrpc": "2.0",
                "id": "req-1",
                "method": "ping"
            }),
        };

        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("\"request_id\":\"req-1\""));
        assert!(json.contains("\"jsonrpc\":\"2.0\""));
    }

    #[test]
    fn test_generate_request_id_unique() {
        let id1 = generate_request_id();
        let id2 = generate_request_id();
        let id3 = generate_request_id();

        // All IDs should be unique
        assert_ne!(id1, id2);
        assert_ne!(id2, id3);
        assert_ne!(id1, id3);
    }

    #[test]
    fn test_generate_request_id_format() {
        let id = generate_request_id();

        // ID should be a string starting with "req-"
        let id_str = id.as_str().unwrap();
        assert!(id_str.starts_with("req-"));
    }

    #[test]
    fn test_get_mcp_methods_returns_methods() {
        let methods = get_mcp_methods();

        // Should return at least the core MCP methods
        assert!(!methods.is_empty());
        assert!(methods.len() >= 5);
    }

    #[test]
    fn test_get_mcp_methods_contains_initialize() {
        let methods = get_mcp_methods();

        let initialize = methods.iter().find(|m| m.method == "initialize");
        assert!(initialize.is_some());

        let init = initialize.unwrap();
        assert!(!init.description.is_empty());
        assert!(init.example_params.is_some());
    }

    #[test]
    fn test_get_mcp_methods_contains_tools_list() {
        let methods = get_mcp_methods();

        let tools_list = methods.iter().find(|m| m.method == "tools/list");
        assert!(tools_list.is_some());
    }

    #[test]
    fn test_get_mcp_methods_contains_ping() {
        let methods = get_mcp_methods();

        let ping = methods.iter().find(|m| m.method == "ping");
        assert!(ping.is_some());
        assert!(ping.unwrap().example_params.is_none());
    }

    #[test]
    fn test_mcp_method_info_serialization() {
        let info = McpMethodInfo {
            method: "tools/call".to_string(),
            description: "Call a tool".to_string(),
            example_params: Some(serde_json::json!({"name": "test_tool"})),
        };

        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("\"method\":\"tools/call\""));
        assert!(json.contains("\"description\":\"Call a tool\""));
    }

    #[test]
    fn test_mcp_method_info_clone() {
        let info = McpMethodInfo {
            method: "ping".to_string(),
            description: "Ping the server".to_string(),
            example_params: None,
        };

        let cloned = info.clone();
        assert_eq!(cloned.method, info.method);
        assert_eq!(cloned.description, info.description);
    }
}
