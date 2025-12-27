//! Storage layer for session recordings using sled
//!
//! This module provides sled-based persistence for recorded sessions,
//! allowing sessions to be saved, loaded, and queried efficiently.

use crate::core::session_recorder::RecordedSession;
use crate::error::{AppError, Result};
use serde::{Deserialize, Serialize};
use sled::Db;
use std::path::PathBuf;
use std::sync::Arc;

/// Session storage using sled embedded database
pub struct SessionStorage {
    db: Arc<Db>,
}

impl SessionStorage {
    /// Create a new session storage
    pub fn new(db_path: PathBuf) -> Result<Self> {
        let db = sled::open(db_path)
            .map_err(|e| AppError::StorageError(format!("Failed to open sled database: {e}")))?;

        Ok(Self { db: Arc::new(db) })
    }

    /// Save a recorded session
    pub async fn save_session(&self, session: &RecordedSession) -> Result<()> {
        let sessions_tree = self
            .db
            .open_tree("sessions")
            .map_err(|e| AppError::StorageError(format!("Failed to open sessions tree: {e}")))?;

        // Serialize session to bytes
        let session_bytes = bincode::serialize(session).map_err(|e| {
            AppError::SerializationError(format!("Failed to serialize session: {e}"))
        })?;

        // Store with session ID as key
        sessions_tree
            .insert(session.id.as_bytes(), session_bytes)
            .map_err(|e| AppError::StorageError(format!("Failed to insert session: {e}")))?;

        // Also store metadata in index tree for efficient listing
        let index_tree = self
            .db
            .open_tree("session_index")
            .map_err(|e| AppError::StorageError(format!("Failed to open index tree: {e}")))?;

        let info = SessionInfo {
            id: session.id.clone(),
            name: session.name.clone(),
            started_at: session.started_at,
            ended_at: session.ended_at,
            message_count: session.metadata.message_count,
            duration_ms: session.metadata.duration_ms,
            transport: session.metadata.transport.clone(),
            server_name: session.metadata.server_id.as_ref().map(|s| s.name.clone()),
            tags: session.metadata.tags.clone(),
        };

        let info_bytes = bincode::serialize(&info)
            .map_err(|e| AppError::SerializationError(format!("Failed to serialize index: {e}")))?;

        // Use timestamp as key for sorted listing
        let key = format!("{:016x}:{}", u64::MAX - session.started_at, session.id);
        index_tree
            .insert(key.as_bytes(), info_bytes)
            .map_err(|e| AppError::StorageError(format!("Failed to insert index: {e}")))?;

        // Flush to disk
        self.db
            .flush_async()
            .await
            .map_err(|e| AppError::StorageError(format!("Failed to flush database: {e}")))?;

        tracing::info!("Saved session {} to sled database", session.id);
        Ok(())
    }

    /// Load a recorded session by ID
    pub async fn load_session(&self, session_id: &str) -> Result<RecordedSession> {
        let sessions_tree = self
            .db
            .open_tree("sessions")
            .map_err(|e| AppError::StorageError(format!("Failed to open sessions tree: {e}")))?;

        let session_bytes = sessions_tree
            .get(session_id.as_bytes())
            .map_err(|e| AppError::StorageError(format!("Failed to get session: {e}")))?
            .ok_or_else(|| AppError::StorageError(format!("Session not found: {session_id}")))?;

        let session: RecordedSession = bincode::deserialize(&session_bytes).map_err(|e| {
            AppError::SerializationError(format!("Failed to deserialize session: {e}"))
        })?;

        Ok(session)
    }

    /// List all recorded sessions (sorted by start time, newest first)
    pub async fn list_sessions(&self) -> Result<Vec<SessionInfo>> {
        let index_tree = self
            .db
            .open_tree("session_index")
            .map_err(|e| AppError::StorageError(format!("Failed to open index tree: {e}")))?;

        let mut sessions = Vec::new();
        let mut seen_ids = std::collections::HashSet::new();

        for item in index_tree.iter() {
            let (_key, value) = item
                .map_err(|e| AppError::StorageError(format!("Failed to iterate sessions: {e}")))?;

            let info: SessionInfo = bincode::deserialize(&value).map_err(|e| {
                AppError::SerializationError(format!("Failed to deserialize index: {e}"))
            })?;

            // Deduplicate by session ID
            if seen_ids.insert(info.id.clone()) {
                sessions.push(info);
            }
        }

        Ok(sessions)
    }

    /// Delete a recorded session
    pub async fn delete_session(&self, session_id: &str) -> Result<()> {
        let sessions_tree = self
            .db
            .open_tree("sessions")
            .map_err(|e| AppError::StorageError(format!("Failed to open sessions tree: {e}")))?;

        // Remove from sessions tree
        sessions_tree
            .remove(session_id.as_bytes())
            .map_err(|e| AppError::StorageError(format!("Failed to remove session: {e}")))?;

        // Remove from index tree
        let index_tree = self
            .db
            .open_tree("session_index")
            .map_err(|e| AppError::StorageError(format!("Failed to open index tree: {e}")))?;

        // Find and remove index entry
        let mut key_to_remove = None;
        for item in index_tree.iter() {
            let (key, value) =
                item.map_err(|e| AppError::StorageError(format!("Failed to iterate index: {e}")))?;

            let info: SessionInfo = bincode::deserialize(&value).map_err(|e| {
                AppError::SerializationError(format!("Failed to deserialize index: {e}"))
            })?;

            if info.id == session_id {
                key_to_remove = Some(key.to_vec());
                break;
            }
        }

        if let Some(key) = key_to_remove {
            index_tree
                .remove(key)
                .map_err(|e| AppError::StorageError(format!("Failed to remove index: {e}")))?;
        }

        // Flush to disk
        self.db
            .flush_async()
            .await
            .map_err(|e| AppError::StorageError(format!("Failed to flush database: {e}")))?;

        tracing::info!("Deleted session {}", session_id);
        Ok(())
    }

    /// Get storage statistics
    #[allow(dead_code)]
    pub fn get_stats(&self) -> Result<StorageStats> {
        let sessions_tree = self
            .db
            .open_tree("sessions")
            .map_err(|e| AppError::StorageError(format!("Failed to open sessions tree: {e}")))?;

        let session_count = sessions_tree.len();
        let db_size = self
            .db
            .size_on_disk()
            .map_err(|e| AppError::StorageError(format!("Failed to get database size: {e}")))?;

        Ok(StorageStats {
            session_count,
            size_bytes: db_size,
        })
    }

    /// List sessions with filtering
    pub async fn list_sessions_filtered(&self, filter: &SessionFilter) -> Result<Vec<SessionInfo>> {
        let all_sessions = self.list_sessions().await?;

        let filtered: Vec<SessionInfo> = all_sessions
            .into_iter()
            .filter(|session| {
                // Filter by server name
                if let Some(ref name) = filter.server_name {
                    if session.server_name.as_ref() != Some(name) {
                        return false;
                    }
                }

                // Filter by transport
                if let Some(ref transport) = filter.transport {
                    if &session.transport != transport {
                        return false;
                    }
                }

                // Filter by tags (session must have ALL specified tags)
                for tag in &filter.tags {
                    if !session.tags.contains(tag) {
                        return false;
                    }
                }

                true
            })
            .collect();

        Ok(filtered)
    }

    /// Add tags to a session
    pub async fn add_session_tags(&self, session_id: &str, tags: Vec<String>) -> Result<()> {
        // Load the session
        let mut session = self.load_session(session_id).await?;

        // Add new tags (deduplicating)
        for tag in tags {
            if !session.metadata.tags.contains(&tag) {
                session.metadata.tags.push(tag);
            }
        }

        // Re-save the session
        self.save_session(&session).await?;

        tracing::info!("Added tags to session {}", session_id);
        Ok(())
    }

    /// Remove tags from a session
    pub async fn remove_session_tags(&self, session_id: &str, tags: Vec<String>) -> Result<()> {
        // Load the session
        let mut session = self.load_session(session_id).await?;

        // Remove specified tags
        session.metadata.tags.retain(|t| !tags.contains(t));

        // Re-save the session
        self.save_session(&session).await?;

        tracing::info!("Removed tags from session {}", session_id);
        Ok(())
    }

    /// Get all unique tags across all sessions
    pub async fn get_all_tags(&self) -> Result<Vec<String>> {
        let sessions = self.list_sessions().await?;
        let mut all_tags: Vec<String> = sessions.into_iter().flat_map(|s| s.tags).collect();

        all_tags.sort();
        all_tags.dedup();

        Ok(all_tags)
    }

    /// Get all unique server names across all sessions
    pub async fn get_all_server_names(&self) -> Result<Vec<String>> {
        let sessions = self.list_sessions().await?;
        let mut server_names: Vec<String> =
            sessions.into_iter().filter_map(|s| s.server_name).collect();

        server_names.sort();
        server_names.dedup();

        Ok(server_names)
    }
}

/// Session information for listing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionInfo {
    pub id: String,
    pub name: String,
    pub started_at: u64,
    pub ended_at: Option<u64>,
    pub message_count: usize,
    pub duration_ms: Option<u64>,
    pub transport: String,
    /// Server name for multi-server filtering
    #[serde(default)]
    pub server_name: Option<String>,
    /// Custom tags for filtering
    #[serde(default)]
    pub tags: Vec<String>,
}

/// Storage statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageStats {
    pub session_count: usize,
    pub size_bytes: u64,
}

/// Filter for querying sessions
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SessionFilter {
    /// Filter by server name
    #[serde(default)]
    pub server_name: Option<String>,
    /// Filter by tags (sessions must have ALL specified tags)
    #[serde(default)]
    pub tags: Vec<String>,
    /// Filter by transport type
    #[serde(default)]
    pub transport: Option<String>,
}

// bincode support - add to dependencies
mod bincode {
    use serde::{Deserialize, Serialize};

    pub fn serialize<T: Serialize>(value: &T) -> Result<Vec<u8>, String> {
        serde_json::to_vec(value).map_err(|e| e.to_string())
    }

    pub fn deserialize<'a, T: Deserialize<'a>>(bytes: &'a [u8]) -> Result<T, String> {
        serde_json::from_slice(bytes).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::session_recorder::{RecordedSession, SessionMetadata};
    use tempfile::TempDir;

    fn create_test_session(id: &str, name: &str) -> RecordedSession {
        RecordedSession {
            id: id.to_string(),
            name: name.to_string(),
            started_at: 1700000000000000,
            ended_at: Some(1700000001000000),
            messages: vec![],
            metadata: SessionMetadata {
                transport: "stdio".to_string(),
                message_count: 5,
                duration_ms: Some(1000),
                client_info: None,
                server_info: None,
                server_id: None,
                tags: vec![],
            },
        }
    }

    #[test]
    fn test_session_info_serialization() {
        let info = SessionInfo {
            id: "session-1".to_string(),
            name: "Test Session".to_string(),
            started_at: 1700000000000000,
            ended_at: Some(1700000001000000),
            message_count: 10,
            duration_ms: Some(1000),
            transport: "stdio".to_string(),
            server_name: Some("test-server".to_string()),
            tags: vec!["tag1".to_string(), "tag2".to_string()],
        };

        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("\"id\":\"session-1\""));
        assert!(json.contains("\"server_name\":\"test-server\""));
        assert!(json.contains("\"tags\":[\"tag1\",\"tag2\"]"));

        let deserialized: SessionInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.id, "session-1");
        assert_eq!(deserialized.tags.len(), 2);
    }

    #[test]
    fn test_session_info_defaults() {
        let json = r#"{
            "id": "session-2",
            "name": "Test",
            "started_at": 1700000000000000,
            "ended_at": null,
            "message_count": 0,
            "duration_ms": null,
            "transport": "sse"
        }"#;

        let info: SessionInfo = serde_json::from_str(json).unwrap();
        assert!(info.server_name.is_none());
        assert!(info.tags.is_empty());
    }

    #[test]
    fn test_storage_stats_serialization() {
        let stats = StorageStats {
            session_count: 42,
            size_bytes: 1024 * 1024,
        };

        let json = serde_json::to_string(&stats).unwrap();
        assert!(json.contains("\"session_count\":42"));
        assert!(json.contains("\"size_bytes\":1048576"));
    }

    #[test]
    fn test_session_filter_default() {
        let filter = SessionFilter::default();

        assert!(filter.server_name.is_none());
        assert!(filter.tags.is_empty());
        assert!(filter.transport.is_none());
    }

    #[test]
    fn test_session_filter_serialization() {
        let filter = SessionFilter {
            server_name: Some("filesystem".to_string()),
            tags: vec!["production".to_string()],
            transport: Some("stdio".to_string()),
        };

        let json = serde_json::to_string(&filter).unwrap();
        let deserialized: SessionFilter = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.server_name, Some("filesystem".to_string()));
        assert_eq!(deserialized.tags, vec!["production".to_string()]);
    }

    #[tokio::test]
    async fn test_session_storage_new() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.db");

        let storage = SessionStorage::new(db_path).unwrap();
        let stats = storage.get_stats().unwrap();

        assert_eq!(stats.session_count, 0);
    }

    #[tokio::test]
    async fn test_session_storage_save_and_load() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.db");
        let storage = SessionStorage::new(db_path).unwrap();

        let session = create_test_session("session-1", "Test Session");
        storage.save_session(&session).await.unwrap();

        let loaded = storage.load_session("session-1").await.unwrap();
        assert_eq!(loaded.id, "session-1");
        assert_eq!(loaded.name, "Test Session");
        assert_eq!(loaded.metadata.message_count, 5);
    }

    #[tokio::test]
    async fn test_session_storage_list_sessions() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.db");
        let storage = SessionStorage::new(db_path).unwrap();

        // Save multiple sessions
        let session1 = create_test_session("session-1", "First");
        let session2 = create_test_session("session-2", "Second");

        storage.save_session(&session1).await.unwrap();
        storage.save_session(&session2).await.unwrap();

        let sessions = storage.list_sessions().await.unwrap();
        assert_eq!(sessions.len(), 2);
    }

    #[tokio::test]
    async fn test_session_storage_delete_session() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.db");
        let storage = SessionStorage::new(db_path).unwrap();

        let session = create_test_session("session-to-delete", "Delete Me");
        storage.save_session(&session).await.unwrap();

        // Verify it exists
        let sessions = storage.list_sessions().await.unwrap();
        assert_eq!(sessions.len(), 1);

        // Delete it
        storage.delete_session("session-to-delete").await.unwrap();

        // Verify it's gone
        let sessions = storage.list_sessions().await.unwrap();
        assert_eq!(sessions.len(), 0);
    }

    #[tokio::test]
    async fn test_session_storage_load_nonexistent() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.db");
        let storage = SessionStorage::new(db_path).unwrap();

        let result = storage.load_session("nonexistent").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_session_storage_tags() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.db");
        let storage = SessionStorage::new(db_path).unwrap();

        let session = create_test_session("session-tags", "Tag Test");
        storage.save_session(&session).await.unwrap();

        // Add tags
        storage
            .add_session_tags(
                "session-tags",
                vec!["prod".to_string(), "debug".to_string()],
            )
            .await
            .unwrap();

        let loaded = storage.load_session("session-tags").await.unwrap();
        assert_eq!(loaded.metadata.tags.len(), 2);
        assert!(loaded.metadata.tags.contains(&"prod".to_string()));

        // Remove a tag
        storage
            .remove_session_tags("session-tags", vec!["debug".to_string()])
            .await
            .unwrap();

        let loaded = storage.load_session("session-tags").await.unwrap();
        assert_eq!(loaded.metadata.tags.len(), 1);
        assert!(!loaded.metadata.tags.contains(&"debug".to_string()));
    }

    #[tokio::test]
    async fn test_session_storage_get_all_tags() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.db");
        let storage = SessionStorage::new(db_path).unwrap();

        let mut session1 = create_test_session("s1", "Session 1");
        session1.metadata.tags = vec!["alpha".to_string(), "beta".to_string()];

        let mut session2 = create_test_session("s2", "Session 2");
        session2.metadata.tags = vec!["beta".to_string(), "gamma".to_string()];

        storage.save_session(&session1).await.unwrap();
        storage.save_session(&session2).await.unwrap();

        let all_tags = storage.get_all_tags().await.unwrap();
        assert_eq!(all_tags.len(), 3); // alpha, beta, gamma (deduplicated)
        assert!(all_tags.contains(&"alpha".to_string()));
        assert!(all_tags.contains(&"beta".to_string()));
        assert!(all_tags.contains(&"gamma".to_string()));
    }

    #[tokio::test]
    async fn test_session_storage_filter_by_transport() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.db");
        let storage = SessionStorage::new(db_path).unwrap();

        let mut session1 = create_test_session("s1", "STDIO Session");
        session1.metadata.transport = "stdio".to_string();

        let mut session2 = create_test_session("s2", "SSE Session");
        session2.metadata.transport = "sse".to_string();

        storage.save_session(&session1).await.unwrap();
        storage.save_session(&session2).await.unwrap();

        let filter = SessionFilter {
            transport: Some("stdio".to_string()),
            ..Default::default()
        };

        let filtered = storage.list_sessions_filtered(&filter).await.unwrap();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].transport, "stdio");
    }

    #[tokio::test]
    async fn test_session_storage_filter_by_tags() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.db");
        let storage = SessionStorage::new(db_path).unwrap();

        let mut session1 = create_test_session("s1", "Tagged");
        session1.metadata.tags = vec!["important".to_string(), "reviewed".to_string()];

        let mut session2 = create_test_session("s2", "Untagged");
        session2.metadata.tags = vec![];

        storage.save_session(&session1).await.unwrap();
        storage.save_session(&session2).await.unwrap();

        let filter = SessionFilter {
            tags: vec!["important".to_string()],
            ..Default::default()
        };

        let filtered = storage.list_sessions_filtered(&filter).await.unwrap();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].id, "s1");
    }

    #[test]
    fn test_bincode_serialize_deserialize() {
        let info = SessionInfo {
            id: "test".to_string(),
            name: "Test".to_string(),
            started_at: 12345,
            ended_at: None,
            message_count: 10,
            duration_ms: Some(100),
            transport: "stdio".to_string(),
            server_name: None,
            tags: vec!["a".to_string()],
        };

        let bytes = bincode::serialize(&info).unwrap();
        let deserialized: SessionInfo = bincode::deserialize(&bytes).unwrap();

        assert_eq!(deserialized.id, "test");
        assert_eq!(deserialized.tags, vec!["a".to_string()]);
    }
}
