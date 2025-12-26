//! Core Reticle functionality
//!
//! This module contains the core proxy implementation and transport abstractions:
//! - `protocol`: MCP protocol message parsing and validation
//! - `proxy`: stdio-based proxy implementation
//! - `sse_proxy`: HTTP/SSE-based proxy implementation (legacy, protocol 2024-11-05)
//! - `streamable_proxy`: Streamable HTTP proxy implementation (protocol 2025-03-26)
//! - `websocket_proxy`: WebSocket proxy implementation for real-time bidirectional communication
//! - `transport`: Transport configuration and error types
//! - `session_recorder`: Session recording and replay
//! - `token_counter`: Token counting and context profiling
//! - `server_analyzer`: MCP server context analysis

pub mod protocol;
pub mod proxy;
pub mod server_analyzer;
pub mod session_recorder;
pub mod sse_proxy;
pub mod streamable_proxy;
pub mod token_counter;
pub mod transport;
pub mod websocket_proxy;

// Re-export core types and functions
pub use proxy::run_proxy;
pub use session_recorder::SessionRecorder;
pub use sse_proxy::start_sse_proxy;
pub use streamable_proxy::start_streamable_proxy;
pub use token_counter::TokenCounter;
pub use transport::TransportConfig;
pub use websocket_proxy::start_websocket_proxy;
