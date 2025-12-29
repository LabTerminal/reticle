//! Session Naming Module
//!
//! Generates beautiful, memorable session names and unique session IDs.
//! Uses a combination of adjectives and nouns to create human-friendly names
//! like "swift-falcon", "cosmic-nebula", "azure-phoenix".

use rand::seq::SliceRandom;
use rand::Rng;
use uuid::Uuid;

/// Adjectives for session names - evocative and memorable
const ADJECTIVES: &[&str] = &[
    // Colors
    "amber", "azure", "coral", "crimson", "cyan", "emerald", "golden", "indigo", "jade", "magenta",
    "obsidian", "ruby", "sapphire", "scarlet", "silver", "violet", // Qualities
    "agile", "bold", "brave", "bright", "calm", "clever", "cosmic", "crystal", "daring", "dynamic",
    "eager", "fierce", "gentle", "grand", "keen", "lively", "mighty", "noble", "prime", "quick",
    "rapid", "serene", "sharp", "silent", "sleek", "smooth", "sonic", "steady", "stellar",
    "subtle", "swift", "vibrant", "vivid", "wild", "wise", "zen", // Tech-inspired
    "binary", "cyber", "digital", "hyper", "nano", "neural", "pixel", "quantum", "turbo", "ultra",
    "virtual", "atomic", "electric", "fusion", "laser", "plasma",
];

/// Nouns for session names - memorable objects and creatures
const NOUNS: &[&str] = &[
    // Animals
    "falcon",
    "phoenix",
    "dragon",
    "tiger",
    "panther",
    "eagle",
    "wolf",
    "hawk",
    "raven",
    "cobra",
    "viper",
    "jaguar",
    "leopard",
    "lynx",
    "orca",
    "shark",
    "dolphin",
    "condor",
    "griffin",
    "sphinx",
    // Space
    "comet",
    "nebula",
    "nova",
    "pulsar",
    "quasar",
    "star",
    "meteor",
    "asteroid",
    "galaxy",
    "cosmos",
    "orbit",
    "eclipse",
    "aurora",
    "horizon",
    "zenith",
    // Tech
    "beacon",
    "circuit",
    "cipher",
    "core",
    "forge",
    "nexus",
    "prism",
    "pulse",
    "relay",
    "signal",
    "spark",
    "surge",
    "vertex",
    "vector",
    "matrix",
    "proxy",
    // Elements
    "flame",
    "frost",
    "storm",
    "thunder",
    "wave",
    "wind",
    "lightning",
    "crystal",
    "ember",
    "glacier",
    "ocean",
    "river",
    "shadow",
    "spark",
    "tide",
    "volt",
];

/// Session identifier with both internal UUID and display name
#[derive(Debug, Clone)]
pub struct SessionId {
    /// Internal unique identifier (UUID v4)
    pub id: String,
    /// Human-friendly display name
    pub name: String,
}

impl SessionId {
    /// Create a new session ID with auto-generated name
    pub fn new() -> Self {
        let id = Uuid::new_v4().to_string();
        let name = generate_session_name();
        Self { id, name }
    }

    /// Create a session ID with a custom name
    pub fn with_name(name: String) -> Self {
        let id = Uuid::new_v4().to_string();
        Self { id, name }
    }

    /// Create a session ID with a custom name, prefixed with server name
    pub fn for_server(server_name: &str) -> Self {
        let id = Uuid::new_v4().to_string();
        let suffix = generate_session_name();
        let name = format!("{server_name}-{suffix}");
        Self { id, name }
    }

    /// Create a session ID from existing values (e.g., from storage)
    pub fn from_parts(id: String, name: String) -> Self {
        Self { id, name }
    }
}

impl Default for SessionId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for SessionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name)
    }
}

/// Generate a beautiful session name like "swift-falcon" or "cosmic-nebula"
pub fn generate_session_name() -> String {
    let mut rng = rand::thread_rng();

    let adjective = ADJECTIVES.choose(&mut rng).unwrap_or(&"swift");
    let noun = NOUNS.choose(&mut rng).unwrap_or(&"session");

    format!("{adjective}-{noun}")
}

/// Generate a session name with a numeric suffix for uniqueness
/// e.g., "swift-falcon-7" or "cosmic-nebula-42"
pub fn generate_session_name_numbered() -> String {
    let mut rng = rand::thread_rng();
    let num: u8 = rng.gen_range(1..100);

    let adjective = ADJECTIVES.choose(&mut rng).unwrap_or(&"swift");
    let noun = NOUNS.choose(&mut rng).unwrap_or(&"session");

    format!("{adjective}-{noun}-{num}")
}

/// Generate a short session ID (8 chars) for display purposes
pub fn generate_short_id() -> String {
    Uuid::new_v4().to_string()[..8].to_string()
}

/// Generate a full UUID v4 session ID
pub fn generate_uuid() -> String {
    Uuid::new_v4().to_string()
}

/// Create a session name from a server name
/// If server_name is provided, uses it as prefix
/// Otherwise generates a beautiful random name
pub fn create_session_name(server_name: Option<&str>) -> String {
    match server_name {
        Some(name) if !name.is_empty() => {
            // Use server name with a short suffix for uniqueness
            let suffix = generate_short_id();
            format!("{}-{}", name, &suffix[..4])
        }
        _ => generate_session_name(),
    }
}

/// Create a full session identifier
pub fn create_session_id(server_name: Option<&str>) -> SessionId {
    let name = create_session_name(server_name);
    SessionId::with_name(name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn test_generate_session_name() {
        let name = generate_session_name();
        assert!(name.contains('-'));

        let parts: Vec<&str> = name.split('-').collect();
        assert_eq!(parts.len(), 2);
        assert!(ADJECTIVES.contains(&parts[0]));
        assert!(NOUNS.contains(&parts[1]));
    }

    #[test]
    fn test_generate_session_name_numbered() {
        let name = generate_session_name_numbered();
        let parts: Vec<&str> = name.split('-').collect();
        assert_eq!(parts.len(), 3);
        assert!(parts[2].parse::<u8>().is_ok());
    }

    #[test]
    fn test_session_id_new() {
        let session = SessionId::new();
        assert!(!session.id.is_empty());
        assert!(!session.name.is_empty());
        assert!(session.name.contains('-'));
    }

    #[test]
    fn test_session_id_with_name() {
        let session = SessionId::with_name("my-custom-session".to_string());
        assert!(!session.id.is_empty());
        assert_eq!(session.name, "my-custom-session");
    }

    #[test]
    fn test_session_id_for_server() {
        let session = SessionId::for_server("github");
        assert!(session.name.starts_with("github-"));
    }

    #[test]
    fn test_create_session_name_with_server() {
        let name = create_session_name(Some("postgres"));
        assert!(name.starts_with("postgres-"));
    }

    #[test]
    fn test_create_session_name_without_server() {
        let name = create_session_name(None);
        assert!(name.contains('-'));
    }

    #[test]
    fn test_uniqueness() {
        let mut names: HashSet<String> = HashSet::new();
        for _ in 0..100 {
            let session = SessionId::new();
            names.insert(session.id);
        }
        // All UUIDs should be unique
        assert_eq!(names.len(), 100);
    }

    #[test]
    fn test_display() {
        let session = SessionId::with_name("test-session".to_string());
        assert_eq!(format!("{session}"), "test-session");
    }
}
