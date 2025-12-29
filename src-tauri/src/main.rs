//! Reticle - Desktop GUI Application
//!
//! A debugging tool for Model Context Protocol (MCP) servers that intercepts
//! and displays JSON-RPC messages in real-time.
//!
//! # Architecture
//!
//! The application consists of:
//! - Tauri backend (Rust) for proxy logic and system integration
//! - React frontend for UI and message visualization
//! - Dual transport support: stdio and HTTP/SSE
//! - Core library (reticle-core) for protocol types and token counting

// Prevents additional console window on Windows in release mode
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod config;
mod core;
mod error;
mod events;
mod mock_data;
mod security;
mod state;
mod storage;

use commands::{
    add_recording_tag, add_session_tags, analyze_mcp_server, can_interact, clear_all_token_stats,
    clear_session_token_stats, delete_recorded_session, estimate_tokens, export_session,
    export_session_csv, export_session_har, get_all_server_names, get_all_tags,
    get_cli_bridge_status, get_cli_sessions, get_global_token_stats, get_mcp_methods,
    get_recording_status, get_recording_tags, get_session_metadata, get_session_token_stats,
    list_recorded_sessions, list_sessions_filtered, load_recorded_session, remove_recording_tag,
    remove_session_tags, send_raw_message, send_request, send_to_cli_session,
    start_cli_bridge_server, start_proxy, start_proxy_v2, start_recording, start_remote_proxy,
    stop_cli_bridge_server, stop_proxy, stop_recording,
};
use core::start_socket_bridge;
use state::AppState;
use tauri::Manager;

fn main() {
    // Initialize tracing for logging
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::INFO.into()),
        )
        .init();

    // Initialize application state with default configuration
    let app_state = AppState::new();

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(app_state)
        .setup(|app| {
            // Auto-start socket bridge for sub-10ms CLI-to-GUI communication
            let app_handle = app.handle().clone();
            let state = app.state::<AppState>();
            let cli_bridge = state.cli_bridge.clone();

            tauri::async_runtime::spawn(async move {
                tracing::info!("Auto-starting socket bridge at /tmp/reticle.sock");

                match start_socket_bridge(app_handle).await {
                    Ok((handle, shutdown_tx)) => {
                        let mut bridge = cli_bridge.lock().await;
                        bridge.is_running = true;
                        bridge.shutdown_tx = Some(shutdown_tx);
                        tracing::info!("Socket bridge started successfully");

                        // Keep the handle alive
                        let _ = handle.await;
                    }
                    Err(e) => {
                        tracing::error!("Failed to start socket bridge: {}", e);
                    }
                }
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            start_proxy,
            stop_proxy,
            start_proxy_v2,
            start_remote_proxy,
            start_recording,
            stop_recording,
            get_recording_status,
            add_recording_tag,
            remove_recording_tag,
            get_recording_tags,
            list_recorded_sessions,
            load_recorded_session,
            delete_recorded_session,
            export_session,
            export_session_csv,
            export_session_har,
            // Interaction commands
            send_request,
            send_raw_message,
            can_interact,
            get_mcp_methods,
            get_cli_sessions,
            send_to_cli_session,
            // Token profiling commands
            get_session_token_stats,
            get_global_token_stats,
            clear_session_token_stats,
            clear_all_token_stats,
            estimate_tokens,
            analyze_mcp_server,
            // Session management commands
            add_session_tags,
            remove_session_tags,
            get_all_tags,
            get_all_server_names,
            list_sessions_filtered,
            get_session_metadata,
            // CLI bridge commands
            start_cli_bridge_server,
            stop_cli_bridge_server,
            get_cli_bridge_status
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
