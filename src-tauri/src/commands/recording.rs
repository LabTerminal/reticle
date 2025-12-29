//! Recording commands for session capture
//!
//! This module provides Tauri commands for controlling session recording,
//! including start/stop recording, listing sessions, and exporting.

use crate::core::session_recorder::SessionRecorder;
use crate::security::generate_secure_session_id;
use crate::state::AppState;
use crate::storage::SessionInfo;
use tauri::State;

/// Start recording a new session
#[tauri::command]
pub async fn start_recording(
    state: State<'_, AppState>,
    session_name: Option<String>,
) -> Result<String, String> {
    let mut recorder_state = state.recorder.lock().await;

    if recorder_state.is_some() {
        return Err("Recording is already active".to_string());
    }

    let session_id = generate_secure_session_id();
    let name = session_name.unwrap_or_else(|| format!("Session {}", chrono_format(&session_id)));

    // Get transport type from proxy state
    let transport_type = "stdio".to_string(); // TODO: Get from actual transport

    let recorder = SessionRecorder::new(session_id.clone(), name, transport_type);

    *recorder_state = Some(recorder);

    tracing::info!("Started recording session: {}", session_id);
    Ok(session_id)
}

/// Stop recording and save the session
#[tauri::command]
pub async fn stop_recording(state: State<'_, AppState>) -> Result<String, String> {
    let mut recorder_state = state.recorder.lock().await;

    if let Some(recorder) = recorder_state.take() {
        let session = recorder
            .finalize()
            .await
            .map_err(|e| format!("Failed to finalize recording: {e}"))?;

        let session_id = session.id.clone();
        let message_count = session.messages.len();

        // Only save sessions with at least one message
        if message_count == 0 {
            tracing::warn!("Discarding empty recording session: {}", session_id);
            return Err("Cannot save empty session (no messages recorded)".to_string());
        }

        // Save to storage
        state
            .storage
            .save_session(&session)
            .await
            .map_err(|e| format!("Failed to save session: {e}"))?;

        tracing::info!(
            "Stopped and saved recording: {} ({} messages)",
            session_id,
            message_count
        );
        Ok(session_id)
    } else {
        Err("No active recording".to_string())
    }
}

/// Get current recording status
#[tauri::command]
pub async fn get_recording_status(state: State<'_, AppState>) -> Result<RecordingStatus, String> {
    let recorder_state = state.recorder.lock().await;

    if let Some(recorder) = recorder_state.as_ref() {
        let stats = recorder.get_stats().await;
        Ok(RecordingStatus {
            is_recording: true,
            session_id: Some(stats.session_id),
            message_count: stats.message_count,
            duration_seconds: stats.duration_seconds,
        })
    } else {
        Ok(RecordingStatus {
            is_recording: false,
            session_id: None,
            message_count: 0,
            duration_seconds: 0,
        })
    }
}

/// List all recorded sessions
#[tauri::command]
pub async fn list_recorded_sessions(
    state: State<'_, AppState>,
) -> Result<Vec<SessionInfo>, String> {
    state
        .storage
        .list_sessions()
        .await
        .map_err(|e| format!("Failed to list sessions: {e}"))
}

/// Load a recorded session
#[tauri::command]
pub async fn load_recorded_session(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<serde_json::Value, String> {
    let session = state
        .storage
        .load_session(&session_id)
        .await
        .map_err(|e| format!("Failed to load session: {e}"))?;

    serde_json::to_value(session).map_err(|e| format!("Failed to serialize session: {e}"))
}

/// Delete a recorded session
#[tauri::command]
pub async fn delete_recorded_session(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<(), String> {
    state
        .storage
        .delete_session(&session_id)
        .await
        .map_err(|e| format!("Failed to delete session: {e}"))
}

/// Export a session to JSON file
#[tauri::command]
pub async fn export_session(
    state: State<'_, AppState>,
    session_id: String,
    export_path: String,
) -> Result<(), String> {
    let session = state
        .storage
        .load_session(&session_id)
        .await
        .map_err(|e| format!("Failed to load session: {e}"))?;

    let json = serde_json::to_string_pretty(&session)
        .map_err(|e| format!("Failed to serialize session: {e}"))?;

    std::fs::write(&export_path, json).map_err(|e| format!("Failed to write export file: {e}"))?;

    tracing::info!("Exported session {} to {}", session_id, export_path);
    Ok(())
}

/// Export a session to CSV file
#[tauri::command]
pub async fn export_session_csv(
    state: State<'_, AppState>,
    session_id: String,
    export_path: String,
) -> Result<(), String> {
    let session = state
        .storage
        .load_session(&session_id)
        .await
        .map_err(|e| format!("Failed to load session: {e}"))?;

    let csv = session_to_csv(&session)?;

    std::fs::write(&export_path, csv).map_err(|e| format!("Failed to write CSV file: {e}"))?;

    tracing::info!("Exported session {} to CSV: {}", session_id, export_path);
    Ok(())
}

/// Export a session to HAR (HTTP Archive) file
#[tauri::command]
pub async fn export_session_har(
    state: State<'_, AppState>,
    session_id: String,
    export_path: String,
) -> Result<(), String> {
    let session = state
        .storage
        .load_session(&session_id)
        .await
        .map_err(|e| format!("Failed to load session: {e}"))?;

    let har = session_to_har(&session)?;

    std::fs::write(&export_path, har).map_err(|e| format!("Failed to write HAR file: {e}"))?;

    tracing::info!("Exported session {} to HAR: {}", session_id, export_path);
    Ok(())
}

/// Convert a session to CSV format
fn session_to_csv(
    session: &reticle_core::session_recorder::RecordedSession,
) -> Result<String, String> {
    use std::fmt::Write;

    let mut csv = String::new();

    // CSV header
    writeln!(
        csv,
        "id,timestamp,relative_time_ms,direction,method,jsonrpc_id,size_bytes,content"
    )
    .map_err(|e| format!("Failed to write CSV header: {e}"))?;

    // CSV rows
    for msg in &session.messages {
        let direction = match msg.direction {
            reticle_core::session_recorder::MessageDirection::ToServer => "request",
            reticle_core::session_recorder::MessageDirection::ToClient => "response",
        };

        let method = msg.metadata.method.as_deref().unwrap_or("");
        let jsonrpc_id = msg
            .metadata
            .jsonrpc_id
            .as_ref()
            .map(|v| v.to_string())
            .unwrap_or_default();

        // Escape content for CSV (double quotes, escape existing quotes)
        let content = msg.content.to_string();
        let escaped_content = content.replace('"', "\"\"");

        writeln!(
            csv,
            "{},{},{},{},{},{},{},\"{}\"",
            msg.id,
            msg.timestamp_micros,
            msg.relative_time_ms,
            direction,
            method,
            jsonrpc_id,
            msg.metadata.size_bytes,
            escaped_content
        )
        .map_err(|e| format!("Failed to write CSV row: {e}"))?;
    }

    Ok(csv)
}

/// Convert a session to HAR (HTTP Archive) format
/// HAR is a standard format for HTTP traffic, we adapt it for MCP/JSON-RPC
fn session_to_har(
    session: &reticle_core::session_recorder::RecordedSession,
) -> Result<String, String> {
    use serde_json::json;

    // Build HAR entries from messages
    // We pair requests with responses based on JSON-RPC ID
    let mut entries: Vec<serde_json::Value> = Vec::new();

    // Create a map of responses by JSON-RPC ID
    let mut response_map: std::collections::HashMap<
        String,
        &reticle_core::session_recorder::RecordedMessage,
    > = std::collections::HashMap::new();

    for msg in &session.messages {
        if matches!(
            msg.direction,
            reticle_core::session_recorder::MessageDirection::ToClient
        ) {
            if let Some(id) = &msg.metadata.jsonrpc_id {
                response_map.insert(id.to_string(), msg);
            }
        }
    }

    // Process requests and pair with responses
    for msg in &session.messages {
        if !matches!(
            msg.direction,
            reticle_core::session_recorder::MessageDirection::ToServer
        ) {
            continue;
        }

        let method = msg.metadata.method.as_deref().unwrap_or("unknown");

        // Find matching response
        let response = msg
            .metadata
            .jsonrpc_id
            .as_ref()
            .and_then(|id| response_map.get(&id.to_string()));

        // Calculate timing
        let wait_time = response
            .map(|r| (r.timestamp_micros - msg.timestamp_micros) / 1000) // Convert to ms
            .unwrap_or(0);

        // Convert timestamp to ISO 8601
        let started_datetime = timestamp_to_iso8601(msg.timestamp_micros);

        // Build HAR entry
        let entry = json!({
            "startedDateTime": started_datetime,
            "time": wait_time,
            "request": {
                "method": "POST",
                "url": format!("mcp://localhost/{}", method),
                "httpVersion": "MCP/1.0",
                "cookies": [],
                "headers": [
                    {"name": "Content-Type", "value": "application/json"}
                ],
                "queryString": [],
                "postData": {
                    "mimeType": "application/json",
                    "text": msg.content.to_string()
                },
                "headersSize": -1,
                "bodySize": msg.metadata.size_bytes
            },
            "response": {
                "status": if response.is_some() { 200 } else { 0 },
                "statusText": if response.is_some() { "OK" } else { "No Response" },
                "httpVersion": "MCP/1.0",
                "cookies": [],
                "headers": [
                    {"name": "Content-Type", "value": "application/json"}
                ],
                "content": {
                    "size": response.map(|r| r.metadata.size_bytes).unwrap_or(0),
                    "mimeType": "application/json",
                    "text": response.map(|r| r.content.to_string()).unwrap_or_default()
                },
                "redirectURL": "",
                "headersSize": -1,
                "bodySize": response.map(|r| r.metadata.size_bytes as i64).unwrap_or(-1)
            },
            "cache": {},
            "timings": {
                "send": 0,
                "wait": wait_time,
                "receive": 0
            },
            "comment": format!("MCP {} call", method)
        });

        entries.push(entry);
    }

    // Build complete HAR structure
    let har = json!({
        "log": {
            "version": "1.2",
            "creator": {
                "name": "Reticle",
                "version": "0.1.0",
                "comment": "MCP Traffic Inspector"
            },
            "browser": {
                "name": "MCP Client",
                "version": "1.0"
            },
            "pages": [
                {
                    "startedDateTime": timestamp_to_iso8601(session.started_at * 1000),
                    "id": &session.id,
                    "title": &session.name,
                    "pageTimings": {
                        "onContentLoad": session.metadata.duration_ms.unwrap_or(0),
                        "onLoad": session.metadata.duration_ms.unwrap_or(0)
                    }
                }
            ],
            "entries": entries,
            "comment": format!(
                "MCP session with {} messages via {} transport",
                session.metadata.message_count,
                session.metadata.transport
            )
        }
    });

    serde_json::to_string_pretty(&har).map_err(|e| format!("Failed to serialize HAR: {e}"))
}

/// Convert microseconds timestamp to ISO 8601 format
fn timestamp_to_iso8601(micros: u64) -> String {
    let secs = micros / 1_000_000;
    let millis = (micros % 1_000_000) / 1000;

    // Calculate date/time components from Unix timestamp
    let days_since_epoch = secs / 86400;
    let time_of_day = secs % 86400;
    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;
    let seconds = time_of_day % 60;

    // Simplified date calculation (not accounting for leap years perfectly)
    let mut year = 1970i64;
    let mut remaining_days = days_since_epoch as i64;

    while remaining_days >= 365 {
        let days_in_year = if is_leap_year(year) { 366 } else { 365 };
        if remaining_days >= days_in_year {
            remaining_days -= days_in_year;
            year += 1;
        } else {
            break;
        }
    }

    let days_in_months = if is_leap_year(year) {
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };

    let mut month = 1;
    for days in days_in_months {
        if remaining_days < days {
            break;
        }
        remaining_days -= days;
        month += 1;
    }
    let day = remaining_days + 1;

    format!("{year:04}-{month:02}-{day:02}T{hours:02}:{minutes:02}:{seconds:02}.{millis:03}Z")
}

fn is_leap_year(year: i64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
}

/// Recording status for UI
#[derive(Debug, serde::Serialize)]
pub struct RecordingStatus {
    pub is_recording: bool,
    pub session_id: Option<String>,
    pub message_count: usize,
    pub duration_seconds: u64,
}

/// Add a tag to the current recording session
#[tauri::command]
pub async fn add_recording_tag(state: State<'_, AppState>, tag: String) -> Result<(), String> {
    let recorder_state = state.recorder.lock().await;

    if let Some(recorder) = recorder_state.as_ref() {
        recorder.add_tag(tag.to_lowercase()).await;
        Ok(())
    } else {
        Err("No active recording".to_string())
    }
}

/// Remove a tag from the current recording session
#[tauri::command]
pub async fn remove_recording_tag(state: State<'_, AppState>, tag: String) -> Result<(), String> {
    let recorder_state = state.recorder.lock().await;

    if let Some(recorder) = recorder_state.as_ref() {
        recorder.remove_tag(&tag).await;
        Ok(())
    } else {
        Err("No active recording".to_string())
    }
}

/// Get tags from the current recording session
#[tauri::command]
pub async fn get_recording_tags(state: State<'_, AppState>) -> Result<Vec<String>, String> {
    let recorder_state = state.recorder.lock().await;

    if let Some(recorder) = recorder_state.as_ref() {
        Ok(recorder.get_tags().await)
    } else {
        Err("No active recording".to_string())
    }
}

// Helper functions

fn chrono_format(session_id: &str) -> String {
    // Extract timestamp from session-{timestamp}-{random} format
    // or legacy session-{timestamp} format
    if let Some(rest) = session_id.strip_prefix("session-") {
        // Get the timestamp part (first segment after "session-")
        let ts_str = rest.split('-').next().unwrap_or(rest);
        if let Ok(ts) = ts_str.parse::<i64>() {
            // Convert timestamp to human-readable format
            // Simple implementation without chrono dependency
            let secs_per_day = 86400;
            let days = ts / secs_per_day;
            let remaining_secs = ts % secs_per_day;
            let hours = remaining_secs / 3600;
            let mins = (remaining_secs % 3600) / 60;
            let secs = remaining_secs % 60;

            // Unix epoch is 1970-01-01, calculate approximate date
            let year = 1970 + (days / 365);
            let month = ((days % 365) / 30) + 1;
            let day = ((days % 365) % 30) + 1;

            return format!("{year:04}-{month:02}-{day:02} {hours:02}:{mins:02}:{secs:02}");
        }
    }
    session_id.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_recording_status_not_recording() {
        let status = RecordingStatus {
            is_recording: false,
            session_id: None,
            message_count: 0,
            duration_seconds: 0,
        };

        let json = serde_json::to_string(&status).unwrap();
        assert!(json.contains("\"is_recording\":false"));
        assert!(json.contains("\"session_id\":null"));
        assert!(json.contains("\"message_count\":0"));
    }

    #[test]
    fn test_recording_status_active() {
        let status = RecordingStatus {
            is_recording: true,
            session_id: Some("session-123".to_string()),
            message_count: 42,
            duration_seconds: 300,
        };

        let json = serde_json::to_string(&status).unwrap();
        assert!(json.contains("\"is_recording\":true"));
        assert!(json.contains("\"session_id\":\"session-123\""));
        assert!(json.contains("\"message_count\":42"));
        assert!(json.contains("\"duration_seconds\":300"));
    }

    #[test]
    fn test_chrono_format_valid_session_id() {
        // Test with a known timestamp: 1700000000 = 2023-11-14 22:13:20 UTC
        let formatted = chrono_format("session-1700000000");

        // Should return a formatted date string
        assert!(formatted.contains("2023")); // Year
        assert!(formatted.contains(":")); // Time separator
    }

    #[test]
    fn test_chrono_format_secure_session_id() {
        // Test with new format: session-{timestamp}-{random}
        let formatted = chrono_format("session-1700000000-abcdef1234567890");

        // Should return a formatted date string (extracts timestamp)
        assert!(formatted.contains("2023")); // Year
        assert!(formatted.contains(":")); // Time separator
    }

    #[test]
    fn test_chrono_format_invalid_session_id() {
        let result = chrono_format("not-a-session-id");

        // Should return the original string
        assert_eq!(result, "not-a-session-id");
    }

    #[test]
    fn test_chrono_format_invalid_number() {
        let result = chrono_format("session-notanumber");

        // Should return the original string
        assert_eq!(result, "session-notanumber");
    }

    #[test]
    fn test_chrono_format_zero_timestamp() {
        let formatted = chrono_format("session-0");

        // Should format to Unix epoch
        assert!(formatted.contains("1970"));
    }

    #[test]
    fn test_recording_status_debug() {
        let status = RecordingStatus {
            is_recording: true,
            session_id: Some("test".to_string()),
            message_count: 10,
            duration_seconds: 60,
        };

        // Debug trait should work
        let debug_str = format!("{status:?}");
        assert!(debug_str.contains("RecordingStatus"));
    }
}
