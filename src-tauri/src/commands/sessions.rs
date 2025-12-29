//! Session management commands for tagging and filtering
//!
//! This module provides Tauri commands for managing session tags,
//! filtering sessions by server and tags, and multi-server support.

use crate::state::AppState;
use crate::storage::{SessionFilter, SessionInfo};
use tauri::State;

/// Add tags to a session
#[tauri::command]
pub async fn add_session_tags(
    state: State<'_, AppState>,
    session_id: String,
    tags: Vec<String>,
) -> Result<(), String> {
    state
        .storage
        .add_session_tags(&session_id, tags)
        .await
        .map_err(|e| format!("Failed to add tags: {e}"))
}

/// Remove tags from a session
#[tauri::command]
pub async fn remove_session_tags(
    state: State<'_, AppState>,
    session_id: String,
    tags: Vec<String>,
) -> Result<(), String> {
    state
        .storage
        .remove_session_tags(&session_id, tags)
        .await
        .map_err(|e| format!("Failed to remove tags: {e}"))
}

/// Get all unique tags across all sessions
#[tauri::command]
pub async fn get_all_tags(state: State<'_, AppState>) -> Result<Vec<String>, String> {
    state
        .storage
        .get_all_tags()
        .await
        .map_err(|e| format!("Failed to get tags: {e}"))
}

/// Get all unique server names across all sessions
#[tauri::command]
pub async fn get_all_server_names(state: State<'_, AppState>) -> Result<Vec<String>, String> {
    state
        .storage
        .get_all_server_names()
        .await
        .map_err(|e| format!("Failed to get server names: {e}"))
}

/// List sessions with filtering by server and/or tags
#[tauri::command]
pub async fn list_sessions_filtered(
    state: State<'_, AppState>,
    filter: SessionFilter,
) -> Result<Vec<SessionInfo>, String> {
    state
        .storage
        .list_sessions_filtered(&filter)
        .await
        .map_err(|e| format!("Failed to filter sessions: {e}"))
}

/// Get session metadata including server info and tags
#[tauri::command]
pub async fn get_session_metadata(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<SessionMetadataResponse, String> {
    let session = state
        .storage
        .load_session(&session_id)
        .await
        .map_err(|e| format!("Failed to load session: {e}"))?;

    Ok(SessionMetadataResponse {
        id: session.id,
        name: session.name,
        started_at: session.started_at,
        ended_at: session.ended_at,
        transport: session.metadata.transport,
        server_name: session.metadata.server_id.as_ref().map(|s| s.name.clone()),
        server_version: session
            .metadata
            .server_id
            .as_ref()
            .and_then(|s| s.version.clone()),
        server_command: session
            .metadata
            .server_id
            .as_ref()
            .map(|s| s.command.clone()),
        connection_type: session
            .metadata
            .server_id
            .as_ref()
            .map(|s| s.connection_type.clone()),
        tags: session.metadata.tags,
        message_count: session.metadata.message_count,
        duration_ms: session.metadata.duration_ms,
    })
}

/// Session metadata response for frontend
#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
pub struct SessionMetadataResponse {
    pub id: String,
    pub name: String,
    pub started_at: u64,
    pub ended_at: Option<u64>,
    pub transport: String,
    pub server_name: Option<String>,
    pub server_version: Option<String>,
    pub server_command: Option<String>,
    pub connection_type: Option<String>,
    pub tags: Vec<String>,
    pub message_count: usize,
    pub duration_ms: Option<u64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_metadata_response_serialization() {
        let response = SessionMetadataResponse {
            id: "session-123".to_string(),
            name: "Test Session".to_string(),
            started_at: 1700000000000,
            ended_at: Some(1700000001000),
            transport: "stdio".to_string(),
            server_name: Some("test-server".to_string()),
            server_version: Some("1.0.0".to_string()),
            server_command: Some("node server.js".to_string()),
            connection_type: Some("stdio".to_string()),
            tags: vec!["production".to_string(), "debug".to_string()],
            message_count: 42,
            duration_ms: Some(1000),
        };

        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("\"id\":\"session-123\""));
        assert!(json.contains("\"name\":\"Test Session\""));
        assert!(json.contains("\"transport\":\"stdio\""));
        assert!(json.contains("\"server_name\":\"test-server\""));
        assert!(json.contains("\"tags\":[\"production\",\"debug\"]"));
        assert!(json.contains("\"message_count\":42"));
    }

    #[test]
    fn test_session_metadata_response_minimal() {
        let response = SessionMetadataResponse {
            id: "session-456".to_string(),
            name: "Minimal".to_string(),
            started_at: 1700000000000,
            ended_at: None,
            transport: "http".to_string(),
            server_name: None,
            server_version: None,
            server_command: None,
            connection_type: None,
            tags: vec![],
            message_count: 0,
            duration_ms: None,
        };

        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("\"ended_at\":null"));
        assert!(json.contains("\"server_name\":null"));
        assert!(json.contains("\"tags\":[]"));
    }

    #[test]
    fn test_session_metadata_response_deserialization() {
        let json = r#"{
            "id": "session-789",
            "name": "Deserialized",
            "started_at": 1700000000000,
            "ended_at": null,
            "transport": "websocket",
            "server_name": "ws-server",
            "server_version": null,
            "server_command": null,
            "connection_type": "websocket",
            "tags": ["test"],
            "message_count": 5,
            "duration_ms": null
        }"#;

        let response: SessionMetadataResponse = serde_json::from_str(json).unwrap();
        assert_eq!(response.id, "session-789");
        assert_eq!(response.transport, "websocket");
        assert_eq!(response.server_name, Some("ws-server".to_string()));
        assert_eq!(response.tags, vec!["test".to_string()]);
    }

    #[test]
    fn test_session_metadata_response_clone() {
        let response = SessionMetadataResponse {
            id: "session-clone".to_string(),
            name: "Clone Test".to_string(),
            started_at: 1700000000000,
            ended_at: None,
            transport: "stdio".to_string(),
            server_name: None,
            server_version: None,
            server_command: None,
            connection_type: None,
            tags: vec!["cloned".to_string()],
            message_count: 10,
            duration_ms: Some(500),
        };

        let cloned = response.clone();
        assert_eq!(cloned.id, response.id);
        assert_eq!(cloned.name, response.name);
        assert_eq!(cloned.tags, response.tags);
    }

    #[test]
    fn test_session_metadata_response_debug() {
        let response = SessionMetadataResponse {
            id: "test".to_string(),
            name: "Debug".to_string(),
            started_at: 0,
            ended_at: None,
            transport: "stdio".to_string(),
            server_name: None,
            server_version: None,
            server_command: None,
            connection_type: None,
            tags: vec![],
            message_count: 0,
            duration_ms: None,
        };

        let debug_str = format!("{response:?}");
        assert!(debug_str.contains("SessionMetadataResponse"));
        assert!(debug_str.contains("id:"));
    }
}
