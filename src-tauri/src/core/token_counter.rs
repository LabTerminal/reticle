//! Token Counter Module
//!
//! Provides token counting functionality for MCP messages to help users
//! understand LLM context consumption. Extracts and counts only the payload
//! content that actually goes to the LLM, not the JSON-RPC protocol overhead.
//!
//! The estimation is based on the cl100k_base tokenizer (used by GPT-4/Claude):
//! - Average of ~4 characters per token for English text
//! - JSON structure adds overhead (brackets, quotes, colons)
//! - Numbers and special characters often become individual tokens
//!
//! LLM-relevant content extraction:
//! - tools/list response: Tool schemas (name, description, inputSchema)
//! - tools/call response: Content array (text, images, etc.)
//! - resources/read response: Resource text content
//! - prompts/get response: Prompt messages
//! - sampling/createMessage request: Messages and system prompt

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

    /// Extract and count tokens for LLM-relevant content from an MCP message.
    ///
    /// This extracts only the payload that actually goes to the LLM context,
    /// not the JSON-RPC protocol overhead.
    pub fn count_mcp_context_tokens(content: &serde_json::Value) -> u64 {
        // Try to extract method from request or find it's a response
        let method = content.get("method").and_then(|m| m.as_str());
        let is_response = content.get("result").is_some() || content.get("error").is_some();

        // For requests, check specific methods
        if let Some(method) = method {
            return Self::count_request_tokens(method, content);
        }

        // For responses, we need to infer what was requested
        // We look at the structure of the result to determine the type
        if is_response {
            return Self::count_response_tokens(content);
        }

        // Fallback: count the whole message (shouldn't happen often)
        Self::count_json_tokens(content)
    }

    /// Count tokens for request payloads
    fn count_request_tokens(method: &str, content: &serde_json::Value) -> u64 {
        match method {
            // sampling/createMessage - messages and systemPrompt go to LLM
            "sampling/createMessage" => {
                let mut tokens = 0u64;

                if let Some(params) = content.get("params") {
                    // Count system prompt
                    if let Some(system) = params.get("systemPrompt").and_then(|s| s.as_str()) {
                        tokens += Self::estimate_tokens(system);
                    }

                    // Count messages
                    if let Some(messages) = params.get("messages").and_then(|m| m.as_array()) {
                        for msg in messages {
                            tokens += Self::count_message_content(msg);
                        }
                    }
                }

                tokens.max(1)
            }

            // tools/call - the arguments are shown to LLM in tool use context
            "tools/call" => {
                let mut tokens = 0u64;

                if let Some(params) = content.get("params") {
                    // Tool name
                    if let Some(name) = params.get("name").and_then(|n| n.as_str()) {
                        tokens += Self::estimate_tokens(name);
                    }

                    // Arguments (serialized)
                    if let Some(args) = params.get("arguments") {
                        tokens += Self::count_json_tokens(args);
                    }
                }

                tokens.max(1)
            }

            // prompts/get - arguments sent to prompt
            "prompts/get" => {
                if let Some(params) = content.get("params") {
                    if let Some(args) = params.get("arguments") {
                        return Self::count_json_tokens(args).max(1);
                    }
                }
                1
            }

            // resources/read - URI is minimal
            "resources/read" => {
                if let Some(params) = content.get("params") {
                    if let Some(uri) = params.get("uri").and_then(|u| u.as_str()) {
                        return Self::estimate_tokens(uri).max(1);
                    }
                }
                1
            }

            // Protocol messages - minimal context impact
            "initialize" | "initialized" | "ping" | "cancelled" => 1,

            // List operations - no content sent to LLM
            "tools/list" | "resources/list" | "prompts/list" | "resources/templates/list" => 1,

            // Other methods - count params if present
            _ => {
                if let Some(params) = content.get("params") {
                    Self::count_json_tokens(params).max(1)
                } else {
                    1
                }
            }
        }
    }

    /// Count tokens for response payloads
    fn count_response_tokens(content: &serde_json::Value) -> u64 {
        // Handle errors - the error message might be shown
        if let Some(error) = content.get("error") {
            if let Some(msg) = error.get("message").and_then(|m| m.as_str()) {
                return Self::estimate_tokens(msg).max(1);
            }
            return 1;
        }

        let result = match content.get("result") {
            Some(r) => r,
            None => return 1,
        };

        // tools/list response - tool definitions go into system prompt
        if let Some(tools) = result.get("tools").and_then(|t| t.as_array()) {
            let mut tokens = 0u64;
            for tool in tools {
                // Name and description
                if let Some(name) = tool.get("name").and_then(|n| n.as_str()) {
                    tokens += Self::estimate_tokens(name);
                }
                if let Some(desc) = tool.get("description").and_then(|d| d.as_str()) {
                    tokens += Self::estimate_tokens(desc);
                }
                // Input schema (important for tool definitions)
                if let Some(schema) = tool.get("inputSchema") {
                    tokens += Self::count_json_tokens(schema);
                }
            }
            return tokens.max(1);
        }

        // tools/call response - content array
        if let Some(content_arr) = result.get("content").and_then(|c| c.as_array()) {
            let mut tokens = 0u64;
            for item in content_arr {
                tokens += Self::count_content_item(item);
            }
            return tokens.max(1);
        }

        // resources/read response - text content
        if let Some(contents) = result.get("contents").and_then(|c| c.as_array()) {
            let mut tokens = 0u64;
            for content_item in contents {
                if let Some(text) = content_item.get("text").and_then(|t| t.as_str()) {
                    tokens += Self::estimate_tokens(text);
                }
                // Blob content is typically base64 - count as roughly 1 token per 4 chars
                if let Some(blob) = content_item.get("blob").and_then(|b| b.as_str()) {
                    tokens += (blob.len() as u64) / 4;
                }
            }
            return tokens.max(1);
        }

        // prompts/list response - prompt definitions
        if let Some(prompts) = result.get("prompts").and_then(|p| p.as_array()) {
            let mut tokens = 0u64;
            for prompt in prompts {
                if let Some(name) = prompt.get("name").and_then(|n| n.as_str()) {
                    tokens += Self::estimate_tokens(name);
                }
                if let Some(desc) = prompt.get("description").and_then(|d| d.as_str()) {
                    tokens += Self::estimate_tokens(desc);
                }
            }
            return tokens.max(1);
        }

        // prompts/get response - messages
        if let Some(messages) = result.get("messages").and_then(|m| m.as_array()) {
            let mut tokens = 0u64;
            for msg in messages {
                tokens += Self::count_message_content(msg);
            }
            return tokens.max(1);
        }

        // resources/list response - resource metadata
        if let Some(resources) = result.get("resources").and_then(|r| r.as_array()) {
            let mut tokens = 0u64;
            for resource in resources {
                if let Some(name) = resource.get("name").and_then(|n| n.as_str()) {
                    tokens += Self::estimate_tokens(name);
                }
                if let Some(desc) = resource.get("description").and_then(|d| d.as_str()) {
                    tokens += Self::estimate_tokens(desc);
                }
            }
            return tokens.max(1);
        }

        // sampling/createMessage response - assistant message
        if result.get("role").is_some() {
            return Self::count_message_content(result);
        }

        // completion/complete response
        if let Some(completion) = result.get("completion") {
            if let Some(values) = completion.get("values").and_then(|v| v.as_array()) {
                let mut tokens = 0u64;
                for val in values {
                    if let Some(s) = val.as_str() {
                        tokens += Self::estimate_tokens(s);
                    }
                }
                return tokens.max(1);
            }
        }

        // Default: minimal for other responses
        1
    }

    /// Count tokens in a message content object
    fn count_message_content(msg: &serde_json::Value) -> u64 {
        let mut tokens = 0u64;

        // Handle content field which can be text or structured
        if let Some(content) = msg.get("content") {
            // Direct text content
            if let Some(text) = content.get("text").and_then(|t| t.as_str()) {
                tokens += Self::estimate_tokens(text);
            }
            // Content type (adds a bit for the type indicator)
            if content.get("type").is_some() {
                tokens += 1;
            }
            // Image content - estimate based on typical token usage
            if content.get("data").is_some() {
                // Images typically use ~85 tokens for low-res, ~765 for high-res
                // Use a middle estimate
                tokens += 200;
            }
        }

        tokens
    }

    /// Count tokens in a content item (from tools/call response)
    fn count_content_item(item: &serde_json::Value) -> u64 {
        let mut tokens = 0u64;

        // Text content
        if let Some(text) = item.get("text").and_then(|t| t.as_str()) {
            tokens += Self::estimate_tokens(text);
        }

        // Image content
        if item.get("data").is_some() {
            tokens += 200; // Estimate for image
        }

        // Resource content (embedded)
        if let Some(resource) = item.get("resource") {
            if let Some(text) = resource.get("text").and_then(|t| t.as_str()) {
                tokens += Self::estimate_tokens(text);
            }
        }

        tokens.max(1)
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

    #[test]
    fn test_count_mcp_context_tokens_tools_list_response() {
        // tools/list response - should count tool schemas
        let response = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "tools": [
                    {
                        "name": "read_file",
                        "description": "Read contents of a file from disk",
                        "inputSchema": {
                            "type": "object",
                            "properties": {
                                "path": { "type": "string", "description": "File path" }
                            }
                        }
                    }
                ]
            }
        });

        let tokens = TokenCounter::count_mcp_context_tokens(&response);
        // Should count: name + description + schema, NOT the jsonrpc/id overhead
        assert!(tokens > 10); // Has meaningful content
        assert!(tokens < 100); // But significantly less than full JSON with protocol overhead

        // Compare to full JSON tokens - should be less
        let full_json_tokens = TokenCounter::count_json_tokens(&response);
        assert!(tokens < full_json_tokens);
    }

    #[test]
    fn test_count_mcp_context_tokens_tools_call_response() {
        // tools/call response - should count content text
        let response = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 5,
            "result": {
                "content": [
                    {
                        "type": "text",
                        "text": "This is the file content that goes to the LLM context."
                    }
                ]
            }
        });

        let tokens = TokenCounter::count_mcp_context_tokens(&response);
        // Should count just the text content
        let text_only = TokenCounter::estimate_tokens(
            "This is the file content that goes to the LLM context.",
        );
        assert_eq!(tokens, text_only);
    }

    #[test]
    fn test_count_mcp_context_tokens_protocol_messages() {
        // Protocol messages should have minimal token count
        let init = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "initialize",
            "params": {
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": { "name": "Test", "version": "1.0" }
            },
            "id": 1
        });

        let tokens = TokenCounter::count_mcp_context_tokens(&init);
        assert_eq!(tokens, 1); // Minimal - protocol overhead doesn't go to LLM
    }

    #[test]
    fn test_count_mcp_context_tokens_sampling_request() {
        // sampling/createMessage - should count messages and system prompt
        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "sampling/createMessage",
            "params": {
                "messages": [
                    {
                        "role": "user",
                        "content": {
                            "type": "text",
                            "text": "What is the capital of France?"
                        }
                    }
                ],
                "systemPrompt": "You are a helpful geography assistant.",
                "maxTokens": 500
            },
            "id": 10
        });

        let tokens = TokenCounter::count_mcp_context_tokens(&request);
        // Should count: system prompt + message text
        let expected_min = TokenCounter::estimate_tokens("You are a helpful geography assistant.")
            + TokenCounter::estimate_tokens("What is the capital of France?");
        assert!(tokens >= expected_min - 2); // Allow some variance
    }

    #[test]
    fn test_count_mcp_context_tokens_resources_read_response() {
        // resources/read - should count the text content
        let response = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 4,
            "result": {
                "contents": [
                    {
                        "uri": "file:///path/to/file.txt",
                        "mimeType": "text/plain",
                        "text": "This is the actual file content that will be included in context."
                    }
                ]
            }
        });

        let tokens = TokenCounter::count_mcp_context_tokens(&response);
        let text_tokens = TokenCounter::estimate_tokens(
            "This is the actual file content that will be included in context.",
        );
        assert_eq!(tokens, text_tokens);
    }
}
