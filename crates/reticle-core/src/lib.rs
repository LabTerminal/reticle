//! Reticle Core Library
//!
//! Core types and utilities for the Reticle MCP debugging proxy.
//! This crate provides the pure Rust components that are independent
//! of any GUI framework (Tauri, etc.).
//!
//! # Modules
//!
//! - [`protocol`] - JSON-RPC protocol types and message handling
//! - [`transport`] - Transport configuration types
//! - [`token_counter`] - Token counting for LLM context profiling
//! - [`session_recorder`] - Session recording and replay
//! - [`storage`] - Persistent storage for sessions
//! - [`events`] - Event sink trait for decoupling from GUI frameworks
//! - [`session_names`] - Beautiful session name generation
//! - [`error`] - Error types

pub mod error;
pub mod events;
pub mod protocol;
pub mod session_names;
pub mod session_recorder;
pub mod storage;
pub mod token_counter;
pub mod transport;

// Re-export commonly used types
pub use error::{AppError, Result};
pub use events::EventSink;
pub use protocol::{Direction, LogEntry, MessageType};
pub use session_names::{create_session_id, create_session_name, generate_session_name, SessionId};
pub use session_recorder::{MessageDirection, RecordedMessage, RecordedSession, SessionRecorder};
pub use storage::{SessionFilter, SessionInfo, SessionStorage};
pub use token_counter::{GlobalTokenStats, SessionTokenStats, TokenCounter};
pub use transport::{TransportConfig, TransportError, TransportType};
