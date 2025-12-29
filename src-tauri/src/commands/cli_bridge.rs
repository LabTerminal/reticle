//! CLI Bridge Commands
//!
//! Tauri commands for managing the CLI bridge WebSocket server.

use tauri::{AppHandle, State};

use crate::core::start_cli_bridge;
use crate::state::AppState;

/// Start the CLI bridge WebSocket server
#[tauri::command]
pub async fn start_cli_bridge_server(
    state: State<'_, AppState>,
    app_handle: AppHandle,
    port: Option<u16>,
) -> Result<u16, String> {
    let mut bridge = state.cli_bridge.lock().await;

    if bridge.is_running {
        return Err("CLI bridge is already running".to_string());
    }

    let port = port.unwrap_or(bridge.port);

    let (_, shutdown_tx) = start_cli_bridge(port, app_handle).await?;

    bridge.is_running = true;
    bridge.port = port;
    bridge.shutdown_tx = Some(shutdown_tx);

    tracing::info!("CLI bridge started on port {}", port);
    Ok(port)
}

/// Stop the CLI bridge WebSocket server
#[tauri::command]
pub async fn stop_cli_bridge_server(state: State<'_, AppState>) -> Result<(), String> {
    let mut bridge = state.cli_bridge.lock().await;

    if !bridge.is_running {
        return Err("CLI bridge is not running".to_string());
    }

    if let Some(tx) = bridge.shutdown_tx.take() {
        let _ = tx.send(());
    }

    bridge.is_running = false;

    tracing::info!("CLI bridge stopped");
    Ok(())
}

/// Get CLI bridge status
#[tauri::command]
pub async fn get_cli_bridge_status(
    state: State<'_, AppState>,
) -> Result<serde_json::Value, String> {
    let bridge = state.cli_bridge.lock().await;

    Ok(serde_json::json!({
        "is_running": bridge.is_running,
        "port": bridge.port,
    }))
}
