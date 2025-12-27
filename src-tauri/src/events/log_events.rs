use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};

use crate::error::{AppError, Result};

/// Log event sent to frontend
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEvent {
    pub id: String,
    pub session_id: String,
    pub timestamp: u64,
    pub direction: String,
    pub content: String,
    pub method: Option<String>,
    pub duration_micros: Option<u64>,
    /// Estimated token count for this message
    #[serde(default)]
    pub token_count: u64,
    /// Server name for multi-server filtering
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server_name: Option<String>,
}

/// Emit a log event to the frontend
pub fn emit_log_event(app_handle: &AppHandle, event: LogEvent) -> Result<()> {
    app_handle
        .emit("log-event", event)
        .map_err(|e| AppError::EventEmissionFailed(e.to_string()))
}
