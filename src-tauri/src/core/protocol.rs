use serde::{Deserialize, Serialize};
use std::fmt;
use std::time::{SystemTime, UNIX_EPOCH};

/// Direction of message flow through the proxy
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Direction {
    /// From host (client) to child (server) - incoming
    #[serde(rename = "in")]
    In,
    /// From child (server) to host (client) - outgoing
    #[serde(rename = "out")]
    Out,
}

/// Type of message content
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum MessageType {
    /// Valid JSON-RPC message
    #[default]
    JsonRpc,
    /// Raw text output (non-JSON from stdout)
    Raw,
    /// Error output from stderr
    Stderr,
}

impl fmt::Display for Direction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Direction::In => write!(f, "in"),
            Direction::Out => write!(f, "out"),
        }
    }
}

/// A logged JSON-RPC message with metadata
/// This matches the frontend LogEntry type
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    /// Unique ID for this log entry
    pub id: String,
    /// Session ID this log belongs to
    pub session_id: String,
    /// When the message was intercepted (microseconds since UNIX_EPOCH)
    pub timestamp: u64,
    /// Direction of the message
    pub direction: Direction,
    /// The JSON-RPC message content as string
    pub content: String,
    /// Optional: extracted method from JSON-RPC
    #[serde(skip_serializing_if = "Option::is_none")]
    pub method: Option<String>,
    /// Optional: processing duration in microseconds
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_micros: Option<u64>,
    /// Type of message content (jsonrpc, raw, stderr)
    #[serde(default)]
    pub message_type: MessageType,
    /// Estimated token count for this message
    #[serde(default)]
    pub token_count: u64,
}

impl LogEntry {
    /// Create a new log entry from a JSON-RPC message
    pub fn new(
        id: String,
        session_id: String,
        direction: Direction,
        content: serde_json::Value,
    ) -> Self {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_micros() as u64;

        let method = extract_method(&content);
        let content_str = serde_json::to_string(&content).unwrap_or_default();

        // Count LLM-relevant tokens (extracts payload, not JSON-RPC overhead)
        let token_count = super::token_counter::TokenCounter::count_mcp_context_tokens(&content);

        Self {
            id,
            session_id,
            timestamp,
            direction,
            content: content_str,
            method,
            duration_micros: None,
            message_type: MessageType::JsonRpc,
            token_count,
        }
    }

    /// Create a new log entry from raw text (non-JSON output)
    pub fn new_raw(
        id: String,
        session_id: String,
        direction: Direction,
        content: String,
        message_type: MessageType,
    ) -> Self {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_micros() as u64;

        // Estimate token count for raw content
        let token_count = super::token_counter::TokenCounter::estimate_tokens(&content);

        Self {
            id,
            session_id,
            timestamp,
            direction,
            content,
            method: None,
            duration_micros: None,
            message_type,
            token_count,
        }
    }
}

/// Extract the method field from a JSON-RPC message if present
fn extract_method(value: &serde_json::Value) -> Option<String> {
    value
        .get("method")
        .and_then(|m| m.as_str())
        .map(|s| s.to_string())
}

/// JSON-RPC 2.0 Request structure
#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub id: Option<serde_json::Value>,
    pub method: String,
    pub params: Option<serde_json::Value>,
}

/// JSON-RPC 2.0 Response structure
#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub id: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

/// JSON-RPC 2.0 Error structure
#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
pub struct JsonRpcError {
    pub code: i64,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

/// JSON-RPC 2.0 Notification structure (no id field)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
pub struct JsonRpcNotification {
    pub jsonrpc: String,
    pub method: String,
    pub params: Option<serde_json::Value>,
}
