use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::config::AppConfig;
use crate::core::{SessionRecorder, TokenCounter};
use crate::state::ProxyState;
use crate::storage::SessionStorage;

/// Global application state
///
/// This is managed by Tauri and shared across all command handlers.
/// Uses Arc<Mutex<>> for thread-safe access.
pub struct AppState {
    /// Proxy runtime state
    pub proxy: Arc<Mutex<ProxyState>>,

    /// Application configuration
    pub config: AppConfig,

    /// Active session recorder (if recording is in progress)
    pub recorder: Arc<Mutex<Option<SessionRecorder>>>,

    /// Session storage backend
    pub storage: Arc<SessionStorage>,

    /// Token counter for context profiling
    pub token_counter: Arc<TokenCounter>,
}

impl AppState {
    /// Create new application state with default config
    pub fn new() -> Self {
        // Initialize storage with default path
        let storage_path = Self::default_storage_path();
        let storage =
            SessionStorage::new(storage_path).expect("Failed to initialize session storage");

        Self {
            proxy: Arc::new(Mutex::new(ProxyState::new())),
            config: AppConfig::new(),
            recorder: Arc::new(Mutex::new(None)),
            storage: Arc::new(storage),
            token_counter: Arc::new(TokenCounter::new()),
        }
    }

    /// Create application state with custom config
    #[allow(dead_code)]
    pub fn with_config(config: AppConfig) -> Self {
        let storage_path = Self::default_storage_path();
        let storage =
            SessionStorage::new(storage_path).expect("Failed to initialize session storage");

        Self {
            proxy: Arc::new(Mutex::new(ProxyState::new())),
            config,
            recorder: Arc::new(Mutex::new(None)),
            storage: Arc::new(storage),
            token_counter: Arc::new(TokenCounter::new()),
        }
    }

    /// Get the default storage path
    fn default_storage_path() -> PathBuf {
        // Use application data directory
        let mut path = dirs::data_dir().unwrap_or_else(|| PathBuf::from("."));
        path.push("mcp-sentinel");
        path.push("sessions.db");
        path
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}
