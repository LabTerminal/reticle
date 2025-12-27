//! Command handlers for Tauri IPC
//!
//! This module contains all Tauri command handlers that can be invoked
//! from the frontend. Commands are grouped by functionality:
//! - `proxy`: Proxy lifecycle management (start, stop, configure)
//! - `demo`: Demo data generation for testing
//! - `recording`: Session recording control and management
//! - `interaction`: Bidirectional MCP communication (send requests)
//! - `tokens`: Token profiling and context statistics
//! - `sessions`: Session tagging and multi-server management

pub mod demo;
pub mod interaction;
pub mod proxy;
pub mod recording;
pub mod sessions;
pub mod tokens;

// Re-export command functions for use in main.rs
pub use interaction::{can_interact, get_mcp_methods, send_raw_message, send_request};
pub use proxy::{start_proxy, start_proxy_v2, start_remote_proxy, stop_proxy};
pub use recording::{
    delete_recorded_session, export_session, get_recording_status, list_recorded_sessions,
    load_recorded_session, start_recording, stop_recording,
};
pub use sessions::{
    add_session_tags, get_all_server_names, get_all_tags, get_session_metadata,
    list_sessions_filtered, remove_session_tags,
};
pub use tokens::{
    analyze_mcp_server, clear_all_token_stats, clear_session_token_stats, estimate_tokens,
    get_global_token_stats, get_session_token_stats,
};
