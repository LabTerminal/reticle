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
        server_version: session.metadata.server_id.as_ref().and_then(|s| s.version.clone()),
        server_command: session.metadata.server_id.as_ref().map(|s| s.command.clone()),
        connection_type: session.metadata.server_id.as_ref().map(|s| s.connection_type.clone()),
        tags: session.metadata.tags,
        message_count: session.metadata.message_count,
        duration_ms: session.metadata.duration_ms,
    })
}

/// Session metadata response for frontend
#[derive(Debug, serde::Serialize)]
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
