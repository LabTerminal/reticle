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
    add_session_tags, analyze_mcp_server, can_interact, clear_all_token_stats,
    clear_session_token_stats, delete_recorded_session, estimate_tokens, export_session,
    get_all_server_names, get_all_tags, get_global_token_stats, get_mcp_methods,
    get_recording_status, get_session_metadata, get_session_token_stats, list_recorded_sessions,
    list_sessions_filtered, load_recorded_session, remove_session_tags, send_raw_message,
    send_request, start_proxy, start_proxy_v2, start_recording, start_remote_proxy, stop_proxy,
    stop_recording,
};
use state::AppState;

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
        .invoke_handler(tauri::generate_handler![
            start_proxy,
            stop_proxy,
            start_proxy_v2,
            start_remote_proxy,
            start_recording,
            stop_recording,
            get_recording_status,
            list_recorded_sessions,
            load_recorded_session,
            delete_recorded_session,
            export_session,
            // Interaction commands
            send_request,
            send_raw_message,
            can_interact,
            get_mcp_methods,
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
            get_session_metadata
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
