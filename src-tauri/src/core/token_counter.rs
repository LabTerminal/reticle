//! Token Counter Module
//!
//! Provides token counting functionality for MCP messages to help users
//! understand context consumption. Uses a simple but reasonably accurate
//! estimation algorithm based on common tokenizer patterns.
//!
//! The estimation is based on the cl100k_base tokenizer (used by GPT-4/Claude):
//! - Average of ~4 characters per token for English text
//! - JSON structure adds overhead (brackets, quotes, colons)
//! - Numbers and special characters often become individual tokens

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Token statistics for a single message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageTokenStats {
    /// Unique message ID
    pub message_id: String,
    /// Method name if available
    pub method: Option<String>,
    /// Estimated token count
    pub token_count: u64,
    /// Character count (for reference)
    pub char_count: u64,
    /// Timestamp in microseconds
    pub timestamp: u64,
}

/// Token statistics for a session
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SessionTokenStats {
    /// Session ID
    pub session_id: String,
    /// Total tokens sent to server (requests)
    pub tokens_to_server: u64,
    /// Total tokens from server (responses)
    pub tokens_from_server: u64,
    /// Total tokens overall
    pub total_tokens: u64,
    /// Token breakdown by method
    pub tokens_by_method: HashMap<String, MethodTokenStats>,
    /// Tool definitions token count (from tools/list response)
    pub tool_definitions_tokens: u64,
    /// Number of tools defined
    pub tool_count: u32,
    /// Prompt definitions token count (from prompts/list response)
    pub prompt_definitions_tokens: u64,
    /// Number of prompts defined
    pub prompt_count: u32,
    /// Resource definitions token count (from resources/list response)
    pub resource_definitions_tokens: u64,
    /// Number of resources defined
    pub resource_count: u32,
}

/// Token statistics per method
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MethodTokenStats {
    /// Total tokens for this method (requests + responses)
    pub total_tokens: u64,
    /// Request tokens
    pub request_tokens: u64,
    /// Response tokens
    pub response_tokens: u64,
    /// Number of calls
    pub call_count: u32,
}

/// Global token statistics across all sessions
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GlobalTokenStats {
    /// Total tokens across all sessions
    pub total_tokens: u64,
    /// Token stats per session
    pub sessions: HashMap<String, SessionTokenStats>,
}

/// Token counter state
pub struct TokenCounter {
    /// Global statistics
    stats: Arc<RwLock<GlobalTokenStats>>,
}

impl TokenCounter {
    /// Create a new token counter
    pub fn new() -> Self {
        Self {
            stats: Arc::new(RwLock::new(GlobalTokenStats::default())),
        }
    }

    /// Estimate token count for a string
    ///
    /// Uses a heuristic based on cl100k_base tokenizer patterns:
    /// - ~4 characters per token for regular text
    /// - JSON punctuation often becomes separate tokens
    /// - Numbers are typically 1-2 tokens per number
    pub fn estimate_tokens(text: &str) -> u64 {
        if text.is_empty() {
            return 0;
        }

        let mut tokens = 0u64;
        let chars: Vec<char> = text.chars().collect();
        let mut i = 0;

        while i < chars.len() {
            let c = chars[i];

            if c.is_whitespace() {
                // Whitespace is often merged with adjacent tokens
                i += 1;
                continue;
            }

            if c == '"' || c == '{' || c == '}' || c == '[' || c == ']' || c == ':' || c == ',' {
                // JSON punctuation - often separate tokens
                tokens += 1;
                i += 1;
                continue;
            }

            if c.is_ascii_digit() || c == '-' || c == '.' {
                // Numbers - count as roughly 1 token per 3 digits
                let mut num_len = 0;
                while i + num_len < chars.len() {
                    let nc = chars[i + num_len];
                    if nc.is_ascii_digit() || nc == '.' || nc == '-' || nc == 'e' || nc == 'E' {
                        num_len += 1;
                    } else {
                        break;
                    }
                }
                if num_len > 0 {
                    tokens += ((num_len as u64) + 2) / 3; // ~3 chars per token for numbers
                    i += num_len;
                    continue;
                }
            }

            // Regular text - count word-like sequences
            let mut word_len = 0;
            while i + word_len < chars.len() {
                let wc = chars[i + word_len];
                if wc.is_alphanumeric() || wc == '_' || wc == '-' {
                    word_len += 1;
                } else {
                    break;
                }
            }

            if word_len > 0 {
                // ~4 characters per token for regular words
                // But common words are often 1 token
                tokens += if word_len <= 4 {
                    1
                } else {
                    ((word_len as u64) + 3) / 4
                };
                i += word_len;
            } else {
                // Single special character
                tokens += 1;
                i += 1;
            }
        }

        // Minimum 1 token for non-empty strings
        tokens.max(1)
    }

    /// Count tokens in a JSON value
    #[allow(dead_code)]
    pub fn count_json_tokens(value: &serde_json::Value) -> u64 {
        let json_str = serde_json::to_string(value).unwrap_or_default();
        Self::estimate_tokens(&json_str)
    }

    /// Record a message and update statistics
    #[allow(dead_code)]
    pub async fn record_message(
        &self,
        session_id: &str,
        message_id: &str,
        content: &serde_json::Value,
        is_request: bool,
    ) -> MessageTokenStats {
        let json_str = serde_json::to_string(content).unwrap_or_default();
        let token_count = Self::estimate_tokens(&json_str);
        let char_count = json_str.len() as u64;

        // Extract method name
        let method = content
            .get("method")
            .and_then(|m| m.as_str())
            .map(|s| s.to_string());

        // Create message stats
        let stats = MessageTokenStats {
            message_id: message_id.to_string(),
            method: method.clone(),
            token_count,
            char_count,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_micros() as u64,
        };

        // Update global stats
        let mut global = self.stats.write().await;
        global.total_tokens += token_count;

        // Get or create session stats
        let session = global
            .sessions
            .entry(session_id.to_string())
            .or_insert_with(|| SessionTokenStats {
                session_id: session_id.to_string(),
                ..Default::default()
            });

        session.total_tokens += token_count;

        if is_request {
            session.tokens_to_server += token_count;
        } else {
            session.tokens_from_server += token_count;
        }

        // Update method stats
        if let Some(ref method_name) = method {
            let method_stats = session
                .tokens_by_method
                .entry(method_name.clone())
                .or_default();

            method_stats.total_tokens += token_count;
            method_stats.call_count += 1;

            if is_request {
                method_stats.request_tokens += token_count;
            } else {
                method_stats.response_tokens += token_count;
            }
        }

        // Check for special responses that define tools/prompts/resources
        if !is_request {
            self.analyze_definition_response(session, content, token_count);
        }

        stats
    }

    /// Analyze response for tool/prompt/resource definitions
    #[allow(dead_code)]
    fn analyze_definition_response(
        &self,
        session: &mut SessionTokenStats,
        content: &serde_json::Value,
        token_count: u64,
    ) {
        // Check if this is a response (has result field, no method)
        if content.get("method").is_some() || content.get("result").is_none() {
            return;
        }

        let result = content.get("result");

        // Check for tools/list response
        if let Some(result) = result {
            if let Some(tools) = result.get("tools").and_then(|t| t.as_array()) {
                session.tool_definitions_tokens = token_count;
                session.tool_count = tools.len() as u32;
            }

            // Check for prompts/list response
            if let Some(prompts) = result.get("prompts").and_then(|p| p.as_array()) {
                session.prompt_definitions_tokens = token_count;
                session.prompt_count = prompts.len() as u32;
            }

            // Check for resources/list response
            if let Some(resources) = result.get("resources").and_then(|r| r.as_array()) {
                session.resource_definitions_tokens = token_count;
                session.resource_count = resources.len() as u32;
            }
        }
    }

    /// Get statistics for a specific session
    pub async fn get_session_stats(&self, session_id: &str) -> Option<SessionTokenStats> {
        let global = self.stats.read().await;
        global.sessions.get(session_id).cloned()
    }

    /// Get global statistics
    pub async fn get_global_stats(&self) -> GlobalTokenStats {
        let global = self.stats.read().await;
        global.clone()
    }

    /// Clear statistics for a session
    pub async fn clear_session(&self, session_id: &str) {
        let mut global = self.stats.write().await;
        if let Some(session) = global.sessions.remove(session_id) {
            global.total_tokens = global.total_tokens.saturating_sub(session.total_tokens);
        }
    }

    /// Clear all statistics
    pub async fn clear_all(&self) {
        let mut global = self.stats.write().await;
        *global = GlobalTokenStats::default();
    }

}

impl Default for TokenCounter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_estimate_tokens_empty() {
        assert_eq!(TokenCounter::estimate_tokens(""), 0);
    }

    #[test]
    fn test_estimate_tokens_simple() {
        // Simple word
        let tokens = TokenCounter::estimate_tokens("hello");
        assert!(tokens >= 1 && tokens <= 2);

        // Simple sentence
        let tokens = TokenCounter::estimate_tokens("Hello world");
        assert!(tokens >= 2 && tokens <= 4);
    }

    #[test]
    fn test_estimate_tokens_json() {
        let json = r#"{"method":"tools/call","params":{"name":"test"}}"#;
        let tokens = TokenCounter::estimate_tokens(json);
        // JSON has lots of punctuation, should be reasonable
        assert!(tokens >= 10 && tokens <= 30);
    }

    #[test]
    fn test_count_json_tokens() {
        let value = serde_json::json!({
            "method": "tools/list",
            "params": {}
        });
        let tokens = TokenCounter::count_json_tokens(&value);
        assert!(tokens > 0);
    }

    #[tokio::test]
    async fn test_record_message() {
        let counter = TokenCounter::new();
        let content = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "tools/call",
            "params": { "name": "test_tool" }
        });

        let stats = counter
            .record_message("session-1", "msg-1", &content, true)
            .await;

        assert_eq!(stats.method, Some("tools/call".to_string()));
        assert!(stats.token_count > 0);

        let session_stats = counter.get_session_stats("session-1").await.unwrap();
        assert_eq!(session_stats.tokens_to_server, stats.token_count);
    }

    #[tokio::test]
    async fn test_tool_definitions_tracking() {
        let counter = TokenCounter::new();

        // Simulate tools/list response
        let response = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "tools": [
                    { "name": "tool1", "description": "First tool" },
                    { "name": "tool2", "description": "Second tool" },
                    { "name": "tool3", "description": "Third tool" }
                ]
            }
        });

        counter
            .record_message("session-1", "msg-1", &response, false)
            .await;

        let session_stats = counter.get_session_stats("session-1").await.unwrap();
        assert_eq!(session_stats.tool_count, 3);
        assert!(session_stats.tool_definitions_tokens > 0);
    }
}
