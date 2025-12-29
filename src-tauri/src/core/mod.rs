//! Core Reticle functionality
//!
//! This module contains the core proxy implementation and transport abstractions:
//! - `protocol`: MCP protocol message parsing and validation (from reticle-core)
//! - `proxy`: stdio-based proxy implementation
//! - `sse_proxy`: HTTP/SSE-based proxy implementation (legacy, protocol 2024-11-05)
//! - `streamable_proxy`: Streamable HTTP proxy implementation (protocol 2025-03-26)
//! - `websocket_proxy`: WebSocket proxy implementation for real-time bidirectional communication
//! - `cli_bridge`: WebSocket server for receiving events from CLI instances (legacy)
//! - `socket_bridge`: Unix socket server for sub-10ms CLI-to-GUI communication
//! - `session_recorder`: Session recording and replay (from reticle-core)
//! - `token_counter`: Token counting and context profiling (from reticle-core)
//! - `server_analyzer`: MCP server context analysis

// Re-export from reticle-core
pub use reticle_core::protocol;
pub use reticle_core::session_recorder;
pub use reticle_core::token_counter;

// Local proxy implementations (use Tauri event emission)
pub mod cli_bridge;
pub mod proxy;
pub mod server_analyzer;
pub mod socket_bridge;
pub mod sse_proxy;
pub mod streamable_proxy;
pub mod websocket_proxy;

// Re-export core types and functions
pub use cli_bridge::start_cli_bridge;
pub use proxy::run_proxy;
pub use reticle_core::session_recorder::SessionRecorder;
pub use reticle_core::token_counter::TokenCounter;
pub use reticle_core::transport::TransportConfig;
pub use socket_bridge::start_socket_bridge;
pub use sse_proxy::start_sse_proxy;
pub use streamable_proxy::start_streamable_proxy;
pub use websocket_proxy::start_websocket_proxy;
