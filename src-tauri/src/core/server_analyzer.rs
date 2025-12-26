//! MCP Server Context Analyzer
//!
//! Analyzes an MCP server to calculate its context token overhead.
//! This helps users understand the "context cost" of connecting a server
//! before using it - useful for identifying context-heavy servers that
//! might bloat an agent's context window.
//!
//! The analyzer connects to the server, fetches all definitions (tools,
//! prompts, resources), and calculates how many tokens they consume.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::time::{timeout, Duration};

use super::token_counter::TokenCounter;

/// Analysis result for an MCP server
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerAnalysis {
    /// Server name from initialize response
    pub server_name: String,
    /// Server version
    pub server_version: String,
    /// Protocol version
    pub protocol_version: String,

    /// Total context tokens for all definitions
    pub total_context_tokens: u64,

    /// Tool analysis
    pub tools: ToolsAnalysis,
    /// Prompts analysis
    pub prompts: PromptsAnalysis,
    /// Resources analysis
    pub resources: ResourcesAnalysis,

    /// Breakdown by category
    pub token_breakdown: HashMap<String, u64>,

    /// Analysis timestamp
    pub analyzed_at: u64,
}

/// Tool definitions analysis
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ToolsAnalysis {
    /// Number of tools
    pub count: u32,
    /// Total tokens for all tool definitions
    pub total_tokens: u64,
    /// Per-tool token breakdown
    pub tools: Vec<ToolTokenInfo>,
}

/// Individual tool token information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolTokenInfo {
    /// Tool name
    pub name: String,
    /// Tool description
    pub description: String,
    /// Tokens for name
    pub name_tokens: u64,
    /// Tokens for description
    pub description_tokens: u64,
    /// Tokens for input schema
    pub schema_tokens: u64,
    /// Total tokens for this tool
    pub total_tokens: u64,
}

/// Prompts analysis
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PromptsAnalysis {
    /// Number of prompts
    pub count: u32,
    /// Total tokens for all prompt definitions
    pub total_tokens: u64,
    /// Per-prompt token breakdown
    pub prompts: Vec<PromptTokenInfo>,
}

/// Individual prompt token information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptTokenInfo {
    /// Prompt name
    pub name: String,
    /// Prompt description
    pub description: Option<String>,
    /// Total tokens for this prompt definition
    pub total_tokens: u64,
}

/// Resources analysis
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ResourcesAnalysis {
    /// Number of resources
    pub count: u32,
    /// Total tokens for all resource definitions
    pub total_tokens: u64,
    /// Per-resource token breakdown
    pub resources: Vec<ResourceTokenInfo>,
}

/// Individual resource token information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceTokenInfo {
    /// Resource URI
    pub uri: String,
    /// Resource name
    pub name: String,
    /// Resource description
    pub description: Option<String>,
    /// Total tokens for this resource definition
    pub total_tokens: u64,
}

/// Error types for server analysis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AnalysisError {
    /// Failed to start the server process
    ProcessStartFailed(String),
    /// Server initialization failed
    InitializationFailed(String),
    /// Timeout waiting for response
    Timeout(String),
    /// Invalid response from server
    InvalidResponse(String),
    /// IO error
    IoError(String),
}

impl std::fmt::Display for AnalysisError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AnalysisError::ProcessStartFailed(msg) => write!(f, "Process start failed: {}", msg),
            AnalysisError::InitializationFailed(msg) => {
                write!(f, "Initialization failed: {}", msg)
            }
            AnalysisError::Timeout(msg) => write!(f, "Timeout: {}", msg),
            AnalysisError::InvalidResponse(msg) => write!(f, "Invalid response: {}", msg),
            AnalysisError::IoError(msg) => write!(f, "IO error: {}", msg),
        }
    }
}

/// MCP Server Analyzer
pub struct ServerAnalyzer {
    /// Command to run the server
    command: String,
    /// Arguments for the server
    args: Vec<String>,
    /// Environment variables
    env: HashMap<String, String>,
    /// Timeout for operations
    timeout_secs: u64,
}

impl ServerAnalyzer {
    /// Create a new analyzer for the given server command
    pub fn new(command: String, args: Vec<String>) -> Self {
        Self {
            command,
            args,
            env: HashMap::new(),
            timeout_secs: 30,
        }
    }

    /// Set environment variables for the server
    pub fn with_env(mut self, env: HashMap<String, String>) -> Self {
        self.env = env;
        self
    }

    /// Set timeout for operations
    pub fn with_timeout(mut self, secs: u64) -> Self {
        self.timeout_secs = secs;
        self
    }

    /// Analyze the MCP server and return context token information
    pub async fn analyze(&self) -> Result<ServerAnalysis, AnalysisError> {
        // Start the server process
        let mut child = self.start_server().await?;

        // Get stdin/stdout handles
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| AnalysisError::IoError("Failed to get stdin".to_string()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| AnalysisError::IoError("Failed to get stdout".to_string()))?;

        let mut writer = stdin;
        let mut reader = BufReader::new(stdout);

        // Initialize the server
        let init_response = self.initialize(&mut writer, &mut reader).await?;

        // Extract server info
        let server_name = init_response
            .get("result")
            .and_then(|r| r.get("serverInfo"))
            .and_then(|s| s.get("name"))
            .and_then(|n| n.as_str())
            .unwrap_or("unknown")
            .to_string();

        let server_version = init_response
            .get("result")
            .and_then(|r| r.get("serverInfo"))
            .and_then(|s| s.get("version"))
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();

        let protocol_version = init_response
            .get("result")
            .and_then(|r| r.get("protocolVersion"))
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();

        // Send initialized notification
        self.send_initialized(&mut writer).await?;

        // Fetch and analyze tools
        let tools = self.analyze_tools(&mut writer, &mut reader).await?;

        // Fetch and analyze prompts
        let prompts = self.analyze_prompts(&mut writer, &mut reader).await?;

        // Fetch and analyze resources
        let resources = self.analyze_resources(&mut writer, &mut reader).await?;

        // Calculate totals
        let total_context_tokens =
            tools.total_tokens + prompts.total_tokens + resources.total_tokens;

        // Build breakdown
        let mut token_breakdown = HashMap::new();
        token_breakdown.insert("tools".to_string(), tools.total_tokens);
        token_breakdown.insert("prompts".to_string(), prompts.total_tokens);
        token_breakdown.insert("resources".to_string(), resources.total_tokens);

        // Clean up
        let _ = child.kill().await;

        Ok(ServerAnalysis {
            server_name,
            server_version,
            protocol_version,
            total_context_tokens,
            tools,
            prompts,
            resources,
            token_breakdown,
            analyzed_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        })
    }

    /// Start the server process
    async fn start_server(&self) -> Result<Child, AnalysisError> {
        let mut cmd = Command::new(&self.command);
        cmd.args(&self.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());

        for (key, value) in &self.env {
            cmd.env(key, value);
        }

        cmd.spawn()
            .map_err(|e| AnalysisError::ProcessStartFailed(e.to_string()))
    }

    /// Send a JSON-RPC message and read the response
    async fn send_request<W, R>(
        &self,
        writer: &mut W,
        reader: &mut BufReader<R>,
        request: serde_json::Value,
    ) -> Result<serde_json::Value, AnalysisError>
    where
        W: AsyncWriteExt + Unpin,
        R: tokio::io::AsyncRead + Unpin,
    {
        // Send request
        let request_str = serde_json::to_string(&request)
            .map_err(|e| AnalysisError::InvalidResponse(e.to_string()))?;

        writer
            .write_all(request_str.as_bytes())
            .await
            .map_err(|e| AnalysisError::IoError(e.to_string()))?;
        writer
            .write_all(b"\n")
            .await
            .map_err(|e| AnalysisError::IoError(e.to_string()))?;
        writer
            .flush()
            .await
            .map_err(|e| AnalysisError::IoError(e.to_string()))?;

        // Read response with timeout
        let mut line = String::new();
        timeout(Duration::from_secs(self.timeout_secs), async {
            reader
                .read_line(&mut line)
                .await
                .map_err(|e| AnalysisError::IoError(e.to_string()))
        })
        .await
        .map_err(|_| AnalysisError::Timeout("Waiting for response".to_string()))??;

        serde_json::from_str(&line).map_err(|e| AnalysisError::InvalidResponse(e.to_string()))
    }

    /// Send a notification (no response expected)
    async fn send_notification<W>(
        &self,
        writer: &mut W,
        notification: serde_json::Value,
    ) -> Result<(), AnalysisError>
    where
        W: AsyncWriteExt + Unpin,
    {
        let notification_str = serde_json::to_string(&notification)
            .map_err(|e| AnalysisError::InvalidResponse(e.to_string()))?;

        writer
            .write_all(notification_str.as_bytes())
            .await
            .map_err(|e| AnalysisError::IoError(e.to_string()))?;
        writer
            .write_all(b"\n")
            .await
            .map_err(|e| AnalysisError::IoError(e.to_string()))?;
        writer
            .flush()
            .await
            .map_err(|e| AnalysisError::IoError(e.to_string()))?;

        Ok(())
    }

    /// Initialize the server
    async fn initialize<W, R>(
        &self,
        writer: &mut W,
        reader: &mut BufReader<R>,
    ) -> Result<serde_json::Value, AnalysisError>
    where
        W: AsyncWriteExt + Unpin,
        R: tokio::io::AsyncRead + Unpin,
    {
        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": {
                    "name": "reticle-analyzer",
                    "version": "1.0.0"
                }
            }
        });

        let response = self.send_request(writer, reader, request).await?;

        if response.get("error").is_some() {
            return Err(AnalysisError::InitializationFailed(
                response
                    .get("error")
                    .and_then(|e| e.get("message"))
                    .and_then(|m| m.as_str())
                    .unwrap_or("Unknown error")
                    .to_string(),
            ));
        }

        Ok(response)
    }

    /// Send initialized notification
    async fn send_initialized<W>(&self, writer: &mut W) -> Result<(), AnalysisError>
    where
        W: AsyncWriteExt + Unpin,
    {
        let notification = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized"
        });

        self.send_notification(writer, notification).await
    }

    /// Analyze tools from the server
    async fn analyze_tools<W, R>(
        &self,
        writer: &mut W,
        reader: &mut BufReader<R>,
    ) -> Result<ToolsAnalysis, AnalysisError>
    where
        W: AsyncWriteExt + Unpin,
        R: tokio::io::AsyncRead + Unpin,
    {
        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/list",
            "params": {}
        });

        let response = self.send_request(writer, reader, request).await?;

        // Handle case where tools/list is not supported
        if response.get("error").is_some() {
            return Ok(ToolsAnalysis::default());
        }

        let tools = response
            .get("result")
            .and_then(|r| r.get("tools"))
            .and_then(|t| t.as_array())
            .cloned()
            .unwrap_or_default();

        let mut analysis = ToolsAnalysis {
            count: tools.len() as u32,
            total_tokens: 0,
            tools: Vec::new(),
        };

        for tool in tools {
            let name = tool
                .get("name")
                .and_then(|n| n.as_str())
                .unwrap_or("")
                .to_string();
            let description = tool
                .get("description")
                .and_then(|d| d.as_str())
                .unwrap_or("")
                .to_string();

            let name_tokens = TokenCounter::estimate_tokens(&name);
            let description_tokens = TokenCounter::estimate_tokens(&description);
            let schema_tokens = tool
                .get("inputSchema")
                .map(|s| TokenCounter::count_json_tokens(s))
                .unwrap_or(0);

            let total_tokens = name_tokens + description_tokens + schema_tokens;
            analysis.total_tokens += total_tokens;

            analysis.tools.push(ToolTokenInfo {
                name,
                description,
                name_tokens,
                description_tokens,
                schema_tokens,
                total_tokens,
            });
        }

        // Sort by token count descending
        analysis.tools.sort_by(|a, b| b.total_tokens.cmp(&a.total_tokens));

        Ok(analysis)
    }

    /// Analyze prompts from the server
    async fn analyze_prompts<W, R>(
        &self,
        writer: &mut W,
        reader: &mut BufReader<R>,
    ) -> Result<PromptsAnalysis, AnalysisError>
    where
        W: AsyncWriteExt + Unpin,
        R: tokio::io::AsyncRead + Unpin,
    {
        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "prompts/list",
            "params": {}
        });

        let response = self.send_request(writer, reader, request).await?;

        // Handle case where prompts/list is not supported
        if response.get("error").is_some() {
            return Ok(PromptsAnalysis::default());
        }

        let prompts = response
            .get("result")
            .and_then(|r| r.get("prompts"))
            .and_then(|p| p.as_array())
            .cloned()
            .unwrap_or_default();

        let mut analysis = PromptsAnalysis {
            count: prompts.len() as u32,
            total_tokens: 0,
            prompts: Vec::new(),
        };

        for prompt in prompts {
            let name = prompt
                .get("name")
                .and_then(|n| n.as_str())
                .unwrap_or("")
                .to_string();
            let description = prompt
                .get("description")
                .and_then(|d| d.as_str())
                .map(|s| s.to_string());

            let name_tokens = TokenCounter::estimate_tokens(&name);
            let description_tokens = description
                .as_ref()
                .map(|d| TokenCounter::estimate_tokens(d))
                .unwrap_or(0);

            // Also count arguments if present
            let args_tokens = prompt
                .get("arguments")
                .and_then(|a| a.as_array())
                .map(|args| {
                    args.iter()
                        .map(|arg| {
                            let arg_name = arg.get("name").and_then(|n| n.as_str()).unwrap_or("");
                            let arg_desc =
                                arg.get("description").and_then(|d| d.as_str()).unwrap_or("");
                            TokenCounter::estimate_tokens(arg_name)
                                + TokenCounter::estimate_tokens(arg_desc)
                        })
                        .sum::<u64>()
                })
                .unwrap_or(0);

            let total_tokens = name_tokens + description_tokens + args_tokens;
            analysis.total_tokens += total_tokens;

            analysis.prompts.push(PromptTokenInfo {
                name,
                description,
                total_tokens,
            });
        }

        // Sort by token count descending
        analysis
            .prompts
            .sort_by(|a, b| b.total_tokens.cmp(&a.total_tokens));

        Ok(analysis)
    }

    /// Analyze resources from the server
    async fn analyze_resources<W, R>(
        &self,
        writer: &mut W,
        reader: &mut BufReader<R>,
    ) -> Result<ResourcesAnalysis, AnalysisError>
    where
        W: AsyncWriteExt + Unpin,
        R: tokio::io::AsyncRead + Unpin,
    {
        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 4,
            "method": "resources/list",
            "params": {}
        });

        let response = self.send_request(writer, reader, request).await?;

        // Handle case where resources/list is not supported
        if response.get("error").is_some() {
            return Ok(ResourcesAnalysis::default());
        }

        let resources = response
            .get("result")
            .and_then(|r| r.get("resources"))
            .and_then(|r| r.as_array())
            .cloned()
            .unwrap_or_default();

        let mut analysis = ResourcesAnalysis {
            count: resources.len() as u32,
            total_tokens: 0,
            resources: Vec::new(),
        };

        for resource in resources {
            let uri = resource
                .get("uri")
                .and_then(|u| u.as_str())
                .unwrap_or("")
                .to_string();
            let name = resource
                .get("name")
                .and_then(|n| n.as_str())
                .unwrap_or("")
                .to_string();
            let description = resource
                .get("description")
                .and_then(|d| d.as_str())
                .map(|s| s.to_string());

            let name_tokens = TokenCounter::estimate_tokens(&name);
            let description_tokens = description
                .as_ref()
                .map(|d| TokenCounter::estimate_tokens(d))
                .unwrap_or(0);

            let total_tokens = name_tokens + description_tokens;
            analysis.total_tokens += total_tokens;

            analysis.resources.push(ResourceTokenInfo {
                uri,
                name,
                description,
                total_tokens,
            });
        }

        // Sort by token count descending
        analysis
            .resources
            .sort_by(|a, b| b.total_tokens.cmp(&a.total_tokens));

        Ok(analysis)
    }
}

/// Convenience function to analyze a server by command
pub async fn analyze_server(
    command: String,
    args: Vec<String>,
    env: Option<HashMap<String, String>>,
    timeout_secs: Option<u64>,
) -> Result<ServerAnalysis, AnalysisError> {
    let mut analyzer = ServerAnalyzer::new(command, args);

    if let Some(env) = env {
        analyzer = analyzer.with_env(env);
    }

    if let Some(timeout) = timeout_secs {
        analyzer = analyzer.with_timeout(timeout);
    }

    analyzer.analyze().await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_token_calculation() {
        let tool = serde_json::json!({
            "name": "read_file",
            "description": "Read the contents of a file from the filesystem",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "The path to the file to read"
                    }
                },
                "required": ["path"]
            }
        });

        let name = tool.get("name").and_then(|n| n.as_str()).unwrap();
        let desc = tool.get("description").and_then(|d| d.as_str()).unwrap();
        let schema = tool.get("inputSchema").unwrap();

        let name_tokens = TokenCounter::estimate_tokens(name);
        let desc_tokens = TokenCounter::estimate_tokens(desc);
        let schema_tokens = TokenCounter::count_json_tokens(schema);

        let total = name_tokens + desc_tokens + schema_tokens;

        // Should have reasonable token counts
        assert!(name_tokens >= 1);
        assert!(desc_tokens >= 5);
        assert!(schema_tokens >= 10);
        assert!(total >= 20);
    }
}
