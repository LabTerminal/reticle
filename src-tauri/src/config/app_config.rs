use serde::{Deserialize, Serialize};

/// Application configuration
///
/// Provides centralized configuration management with:
/// - Serde support for loading from files/env
/// - Builder pattern for customization
/// - Sensible defaults
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppConfig {
    /// Demo mode settings
    pub demo: DemoConfig,
    /// Security settings
    #[serde(default)]
    pub security: SecurityConfig,
}

/// Security configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityConfig {
    /// Allowed commands that can be executed as MCP servers.
    /// If empty, all commands are allowed (less secure, development mode).
    /// For production, specify allowed executables like ["npx", "node", "python", "uvx"].
    pub allowed_commands: Vec<String>,

    /// Allowed CORS origins for HTTP proxies.
    /// If empty, defaults to localhost only for security.
    /// Use ["*"] to allow all origins (not recommended for production).
    pub cors_origins: Vec<String>,
}

impl Default for SecurityConfig {
    fn default() -> Self {
        Self {
            // Default to common MCP server launchers
            allowed_commands: vec![
                "npx".to_string(),
                "node".to_string(),
                "python".to_string(),
                "python3".to_string(),
                "uvx".to_string(),
                "uv".to_string(),
                "deno".to_string(),
                "bun".to_string(),
            ],
            // Default to localhost origins only
            cors_origins: vec![
                "http://localhost".to_string(),
                "http://127.0.0.1".to_string(),
                "tauri://localhost".to_string(),
            ],
        }
    }
}

impl SecurityConfig {
    /// Check if a command is allowed to be executed
    pub fn is_command_allowed(&self, command: &str) -> bool {
        // If allowlist is empty, allow all (development mode)
        if self.allowed_commands.is_empty() {
            return true;
        }

        // Extract the base command name (handle paths like /usr/bin/python)
        let base_command = std::path::Path::new(command)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or(command);

        self.allowed_commands.iter().any(|allowed| {
            let allowed_base = std::path::Path::new(allowed)
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or(allowed);
            base_command == allowed_base
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DemoConfig {
    /// Session ID to use for demo mode
    pub session_id: String,

    /// Delay between emitting messages (milliseconds)
    pub message_delay_ms: u64,

    /// How often to log progress (every N messages)
    pub progress_batch_size: usize,

    /// Delay before starting to emit messages (milliseconds)
    pub startup_delay_ms: u64,
}

impl Default for DemoConfig {
    fn default() -> Self {
        Self {
            session_id: super::defaults::DEFAULT_DEMO_SESSION_ID.to_string(),
            message_delay_ms: super::defaults::DEFAULT_DEMO_MESSAGE_DELAY_MS,
            progress_batch_size: super::defaults::DEFAULT_DEMO_PROGRESS_BATCH,
            startup_delay_ms: super::defaults::DEFAULT_SESSION_STARTUP_DELAY_MS,
        }
    }
}

impl AppConfig {
    /// Create a new configuration with defaults
    pub fn new() -> Self {
        Self::default()
    }

    /// Builder method for demo configuration
    #[allow(dead_code)]
    pub fn with_demo_config(mut self, config: DemoConfig) -> Self {
        self.demo = config;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_app_config_default() {
        let config = AppConfig::default();

        assert_eq!(config.demo.session_id, "demo-session-12345");
        assert_eq!(config.demo.message_delay_ms, 10);
        assert_eq!(config.demo.progress_batch_size, 10);
        assert_eq!(config.demo.startup_delay_ms, 100);
    }

    #[test]
    fn test_app_config_new() {
        let config = AppConfig::new();

        assert_eq!(config.demo.session_id, "demo-session-12345");
    }

    #[test]
    fn test_demo_config_default() {
        let demo_config = DemoConfig::default();

        assert_eq!(
            demo_config.session_id,
            super::super::defaults::DEFAULT_DEMO_SESSION_ID
        );
        assert_eq!(
            demo_config.message_delay_ms,
            super::super::defaults::DEFAULT_DEMO_MESSAGE_DELAY_MS
        );
    }

    #[test]
    fn test_app_config_with_demo_config() {
        let custom_demo = DemoConfig {
            session_id: "custom-session".to_string(),
            message_delay_ms: 50,
            progress_batch_size: 5,
            startup_delay_ms: 200,
        };

        let config = AppConfig::new().with_demo_config(custom_demo);

        assert_eq!(config.demo.session_id, "custom-session");
        assert_eq!(config.demo.message_delay_ms, 50);
        assert_eq!(config.demo.progress_batch_size, 5);
        assert_eq!(config.demo.startup_delay_ms, 200);
    }

    #[test]
    fn test_app_config_serialization() {
        let config = AppConfig::default();
        let json = serde_json::to_string(&config).unwrap();

        assert!(json.contains("\"session_id\""));
        assert!(json.contains("\"message_delay_ms\""));
        assert!(json.contains("demo-session-12345"));
    }

    #[test]
    fn test_app_config_deserialization() {
        let json = r#"{
            "demo": {
                "session_id": "test-session",
                "message_delay_ms": 20,
                "progress_batch_size": 15,
                "startup_delay_ms": 150
            }
        }"#;

        let config: AppConfig = serde_json::from_str(json).unwrap();

        assert_eq!(config.demo.session_id, "test-session");
        assert_eq!(config.demo.message_delay_ms, 20);
        assert_eq!(config.demo.progress_batch_size, 15);
        assert_eq!(config.demo.startup_delay_ms, 150);
    }

    #[test]
    fn test_demo_config_clone() {
        let demo1 = DemoConfig::default();
        let demo2 = demo1.clone();

        assert_eq!(demo1.session_id, demo2.session_id);
        assert_eq!(demo1.message_delay_ms, demo2.message_delay_ms);
    }

    #[test]
    fn test_security_config_default() {
        let config = SecurityConfig::default();

        assert!(!config.allowed_commands.is_empty());
        assert!(config.allowed_commands.contains(&"npx".to_string()));
        assert!(config.allowed_commands.contains(&"python".to_string()));
        assert!(!config.cors_origins.is_empty());
    }

    #[test]
    fn test_is_command_allowed_basic() {
        let config = SecurityConfig::default();

        assert!(config.is_command_allowed("npx"));
        assert!(config.is_command_allowed("node"));
        assert!(config.is_command_allowed("python"));
        assert!(!config.is_command_allowed("rm"));
        assert!(!config.is_command_allowed("curl"));
    }

    #[test]
    fn test_is_command_allowed_with_path() {
        let config = SecurityConfig::default();

        assert!(config.is_command_allowed("/usr/bin/python"));
        assert!(config.is_command_allowed("/usr/local/bin/npx"));
        assert!(!config.is_command_allowed("/bin/rm"));
    }

    #[test]
    fn test_is_command_allowed_empty_allowlist() {
        let config = SecurityConfig {
            allowed_commands: vec![],
            cors_origins: vec![],
        };

        // Empty allowlist should allow all commands (dev mode)
        assert!(config.is_command_allowed("anything"));
        assert!(config.is_command_allowed("rm"));
    }

    #[test]
    fn test_app_config_has_security() {
        let config = AppConfig::default();

        assert!(!config.security.allowed_commands.is_empty());
    }
}
