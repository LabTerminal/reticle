//! Token profiling commands
//!
//! Tauri commands for accessing token statistics and context profiling data.

use tauri::State;

use crate::core::token_counter::{GlobalTokenStats, SessionTokenStats};
use crate::state::AppState;

/// Get token statistics for a specific session
#[tauri::command]
pub async fn get_session_token_stats(
    session_id: String,
    state: State<'_, AppState>,
) -> Result<Option<SessionTokenStats>, String> {
    Ok(state.token_counter.get_session_stats(&session_id).await)
}

/// Get global token statistics across all sessions
#[tauri::command]
pub async fn get_global_token_stats(
    state: State<'_, AppState>,
) -> Result<GlobalTokenStats, String> {
    Ok(state.token_counter.get_global_stats().await)
}

/// Clear token statistics for a specific session
#[tauri::command]
pub async fn clear_session_token_stats(
    session_id: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    state.token_counter.clear_session(&session_id).await;
    Ok(())
}

/// Clear all token statistics
#[tauri::command]
pub async fn clear_all_token_stats(state: State<'_, AppState>) -> Result<(), String> {
    state.token_counter.clear_all().await;
    Ok(())
}

/// Estimate tokens for a given text (utility function)
#[tauri::command]
pub fn estimate_tokens(text: String) -> u64 {
    crate::core::TokenCounter::estimate_tokens(&text)
}
