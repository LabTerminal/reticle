use crate::core::TokenCounter;
use crate::events::{LogEvent, SessionStartEvent};

/// Pre-filled mock data for demo mode
pub struct MockData {
    pub session: SessionStartEvent,
    pub logs: Vec<LogEvent>,
}

impl MockData {
    pub fn generate() -> Self {
        let session_id = "demo-session-12345".to_string();
        let base_timestamp = 1734720000000000u64; // Fixed timestamp for consistency

        let session = SessionStartEvent {
            id: session_id.clone(),
            started_at: base_timestamp,
        };

        let mut logs = Vec::new();
        let mut log_id_counter = 0;

        // Helper to create log entries with LLM-relevant token counting
        let mut add_log = |offset_ms: u64,
                           direction: &str,
                           content: String,
                           method: Option<&str>,
                           duration: Option<u64>| {
            // Parse JSON to count LLM-relevant tokens (not raw JSON-RPC overhead)
            let token_count = if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content)
            {
                TokenCounter::count_mcp_context_tokens(&json)
            } else {
                TokenCounter::estimate_tokens(&content)
            };
            logs.push(LogEvent {
                id: format!("log-{log_id_counter}"),
                session_id: session_id.clone(),
                timestamp: base_timestamp + (offset_ms * 1000),
                direction: direction.to_string(),
                content,
                method: method.map(|s| s.to_string()),
                duration_micros: duration,
                token_count,
            });
            log_id_counter += 1;
        };

        // 1. Initialize handshake
        add_log(0, "in", r#"{"jsonrpc":"2.0","method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{"roots":{"listChanged":true},"sampling":{}},"clientInfo":{"name":"Claude Desktop","version":"1.0.0"}},"id":1}"#.to_string(), Some("initialize"), None);
        add_log(150, "out", r#"{"jsonrpc":"2.0","result":{"protocolVersion":"2024-11-05","capabilities":{"logging":{},"prompts":{"listChanged":true},"resources":{"subscribe":true,"listChanged":true},"tools":{"listChanged":true}},"serverInfo":{"name":"filesystem-server","version":"0.1.0"}},"id":1}"#.to_string(), None, Some(150000));

        add_log(
            200,
            "in",
            r#"{"jsonrpc":"2.0","method":"initialized","params":{}}"#.to_string(),
            Some("initialized"),
            None,
        );
        add_log(
            210,
            "out",
            r#"{"jsonrpc":"2.0","result":{"status":"ok"}}"#.to_string(),
            None,
            Some(10000),
        );

        // 2. List available tools
        add_log(
            300,
            "in",
            r#"{"jsonrpc":"2.0","method":"tools/list","params":{},"id":2}"#.to_string(),
            Some("tools/list"),
            None,
        );
        add_log(350, "out", r#"{"jsonrpc":"2.0","result":{"tools":[{"name":"read_file","description":"Read contents of a file","inputSchema":{"type":"object","properties":{"path":{"type":"string","description":"File path"}}}},{"name":"write_file","description":"Write content to a file","inputSchema":{"type":"object","properties":{"path":{"type":"string"},"content":{"type":"string"}}}},{"name":"list_directory","description":"List directory contents","inputSchema":{"type":"object","properties":{"path":{"type":"string"}}}},{"name":"search_files","description":"Search for files matching a pattern","inputSchema":{"type":"object","properties":{"pattern":{"type":"string"},"directory":{"type":"string"}}}}]},"id":2}"#.to_string(), None, Some(50000));

        // 3. List resources
        add_log(
            400,
            "in",
            r#"{"jsonrpc":"2.0","method":"resources/list","params":{},"id":3}"#.to_string(),
            Some("resources/list"),
            None,
        );
        add_log(450, "out", r#"{"jsonrpc":"2.0","result":{"resources":[{"uri":"file:///Users/demo/project/README.md","name":"README.md","description":"Project documentation","mimeType":"text/markdown"},{"uri":"file:///Users/demo/project/src/main.rs","name":"main.rs","mimeType":"text/x-rust"},{"uri":"file:///Users/demo/project/Cargo.toml","name":"Cargo.toml","mimeType":"text/x-toml"}]},"id":3}"#.to_string(), None, Some(50000));

        // 4. Read a resource
        add_log(500, "in", r#"{"jsonrpc":"2.0","method":"resources/read","params":{"uri":"file:///Users/demo/project/README.md"},"id":4}"#.to_string(), Some("resources/read"), None);
        add_log(750, "out", r#"{"jsonrpc":"2.0","result":{"contents":[{"uri":"file:///Users/demo/project/README.md","mimeType":"text/markdown","text":"Demo Project Documentation"}]},"id":4}"#.to_string(), None, Some(250000));

        // 5. Call a tool - list directory
        add_log(800, "in", r#"{"jsonrpc":"2.0","method":"tools/call","params":{"name":"list_directory","arguments":{"path":"/Users/demo/project/src"}},"id":5}"#.to_string(), Some("tools/call"), None);
        add_log(1100, "out", r#"{"jsonrpc":"2.0","result":{"content":[{"type":"text","text":"main.rs lib.rs utils.rs tests.rs"}]},"id":5}"#.to_string(), None, Some(300000));

        // 6. Call a tool - read file
        add_log(1200, "in", r#"{"jsonrpc":"2.0","method":"tools/call","params":{"name":"read_file","arguments":{"path":"/Users/demo/project/src/main.rs"}},"id":6}"#.to_string(), Some("tools/call"), None);
        add_log(1650, "out", r#"{"jsonrpc":"2.0","result":{"content":[{"type":"text","text":"fn main() { println!(\"Hello MCP\"); }"}]},"id":6}"#.to_string(), None, Some(450000));

        // 7. List prompts
        add_log(
            1700,
            "in",
            r#"{"jsonrpc":"2.0","method":"prompts/list","params":{},"id":7}"#.to_string(),
            Some("prompts/list"),
            None,
        );
        add_log(1750, "out", r#"{"jsonrpc":"2.0","result":{"prompts":[{"name":"code_review","description":"Review code for best practices","arguments":[{"name":"code","description":"Code to review","required":true}]},{"name":"explain_code","description":"Explain what code does","arguments":[{"name":"code","description":"Code to explain","required":true}]}]},"id":7}"#.to_string(), None, Some(50000));

        // 8. Get a prompt
        add_log(1800, "in", r#"{"jsonrpc":"2.0","method":"prompts/get","params":{"name":"code_review","arguments":{"code":"fn test() { return 42; }"}},"id":8}"#.to_string(), Some("prompts/get"), None);
        add_log(1900, "out", r#"{"jsonrpc":"2.0","result":{"description":"Code review prompt","messages":[{"role":"user","content":{"type":"text","text":"Please review this code for quality and best practices"}}]},"id":8}"#.to_string(), None, Some(100000));

        // 9. Sampling request (LLM call)
        add_log(2000, "in", r#"{"jsonrpc":"2.0","method":"sampling/createMessage","params":{"messages":[{"role":"user","content":{"type":"text","text":"Analyze this Rust code and suggest improvements"}}],"systemPrompt":"You are a helpful Rust expert","maxTokens":500},"id":9}"#.to_string(), Some("sampling/createMessage"), None);
        add_log(4500, "out", r#"{"jsonrpc":"2.0","result":{"role":"assistant","content":{"type":"text","text":"Great code! Consider adding error handling and documentation."},"model":"claude-3-5-sonnet-20241022","stopReason":"end_turn"},"id":9}"#.to_string(), None, Some(2500000));

        // 10. Subscribe to resource updates
        add_log(4600, "in", r#"{"jsonrpc":"2.0","method":"resources/subscribe","params":{"uri":"file:///Users/demo/project/src/main.rs"},"id":10}"#.to_string(), Some("resources/subscribe"), None);
        add_log(
            4650,
            "out",
            r#"{"jsonrpc":"2.0","result":{"subscribed":true},"id":10}"#.to_string(),
            None,
            Some(50000),
        );

        // 11. Resource update notification
        add_log(5000, "out", r#"{"jsonrpc":"2.0","method":"notifications/resources/updated","params":{"uri":"file:///Users/demo/project/src/main.rs"}}"#.to_string(), None, Some(0));

        // 12. Search files tool
        add_log(5100, "in", r#"{"jsonrpc":"2.0","method":"tools/call","params":{"name":"search_files","arguments":{"pattern":"*.rs","directory":"/Users/demo/project"}},"id":11}"#.to_string(), Some("tools/call"), None);
        add_log(5800, "out", r#"{"jsonrpc":"2.0","result":{"content":[{"type":"text","text":"Found 12 Rust files in project"}]},"id":11}"#.to_string(), None, Some(700000));

        // 13. Multiple rapid tool calls
        for i in 0..15 {
            let ts = 6000 + (i * 150);
            add_log(
                ts,
                "in",
                format!(
                    r#"{{"jsonrpc":"2.0","method":"tools/call","params":{{"name":"read_file","arguments":{{"path":"/Users/demo/file{}.txt"}}}},"id":{}}}"#,
                    i,
                    12 + i
                ),
                Some("tools/call"),
                None,
            );
            add_log(
                ts + 80,
                "out",
                format!(
                    r#"{{"jsonrpc":"2.0","result":{{"content":[{{"type":"text","text":"Content of file {} - Lorem ipsum dolor sit amet"}}]}},"id":{}}}"#,
                    i,
                    12 + i
                ),
                None,
                Some(80000),
            );
        }

        // 14. Error example - file not found
        add_log(8500, "in", r#"{"jsonrpc":"2.0","method":"tools/call","params":{"name":"read_file","arguments":{"path":"/nonexistent/file.txt"}},"id":27}"#.to_string(), Some("tools/call"), None);
        add_log(8700, "out", r#"{"jsonrpc":"2.0","error":{"code":-32603,"message":"File not found","data":{"path":"/nonexistent/file.txt","reason":"No such file or directory"}},"id":27}"#.to_string(), None, Some(200000));

        // 15. Error example - invalid arguments
        add_log(9000, "in", r#"{"jsonrpc":"2.0","method":"tools/call","params":{"name":"read_file","arguments":{}},"id":28}"#.to_string(), Some("tools/call"), None);
        add_log(9050, "out", r#"{"jsonrpc":"2.0","error":{"code":-32602,"message":"Invalid params","data":{"reason":"Missing required argument: path"}},"id":28}"#.to_string(), None, Some(50000));

        // 16. Completion requests
        add_log(9200, "in", r#"{"jsonrpc":"2.0","method":"completion/complete","params":{"ref":{"type":"ref/prompt","name":"code_review"},"argument":{"name":"code","value":"fn te"}},"id":29}"#.to_string(), Some("completion/complete"), None);
        add_log(9350, "out", r#"{"jsonrpc":"2.0","result":{"completion":{"values":["test","temperature","tensor"],"total":3,"hasMore":false}},"id":29}"#.to_string(), None, Some(150000));

        // 17. Logging level change
        add_log(
            9500,
            "in",
            r#"{"jsonrpc":"2.0","method":"logging/setLevel","params":{"level":"debug"},"id":30}"#
                .to_string(),
            Some("logging/setLevel"),
            None,
        );
        add_log(
            9520,
            "out",
            r#"{"jsonrpc":"2.0","result":{"level":"debug"},"id":30}"#.to_string(),
            None,
            Some(20000),
        );

        // 18. More complex sampling with image
        add_log(10000, "in", r#"{"jsonrpc":"2.0","method":"sampling/createMessage","params":{"messages":[{"role":"user","content":{"type":"text","text":"What's in this image?"}},{"role":"user","content":{"type":"image","data":"base64encodeddata...","mimeType":"image/png"}}],"maxTokens":1000},"id":31}"#.to_string(), Some("sampling/createMessage"), None);
        add_log(13000, "out", r#"{"jsonrpc":"2.0","result":{"role":"assistant","content":{"type":"text","text":"I can see a beautiful landscape with mountains in the background, a serene lake in the foreground, and pine trees dotting the hillside. The image appears to be taken during golden hour with warm lighting."},"model":"claude-3-5-sonnet-20241022","stopReason":"end_turn"},"id":31}"#.to_string(), None, Some(3000000));

        // 19. Write file tool
        add_log(13200, "in", r#"{"jsonrpc":"2.0","method":"tools/call","params":{"name":"write_file","arguments":{"path":"/Users/demo/output.txt","content":"Analysis complete!\n\nResults:\n- Performance: Excellent\n- Code quality: High\n- Test coverage: 95%"}},"id":32}"#.to_string(), Some("tools/call"), None);
        add_log(13600, "out", r#"{"jsonrpc":"2.0","result":{"content":[{"type":"text","text":"Successfully wrote 98 bytes to /Users/demo/output.txt"}]},"id":32}"#.to_string(), None, Some(400000));

        // 20. Final batch of reads with varying latencies
        for i in 0..25 {
            let ts = 14000 + (i * 100);
            let latency = if i % 3 == 0 {
                500000
            } else if i % 3 == 1 {
                200000
            } else {
                800000
            };
            add_log(
                ts,
                "in",
                format!(
                    r#"{{"jsonrpc":"2.0","method":"resources/read","params":{{"uri":"file:///Users/demo/data/record{}.json"}},"id":{}}}"#,
                    i,
                    33 + i
                ),
                Some("resources/read"),
                None,
            );
            add_log(
                ts + (latency / 1000),
                "out",
                format!(
                    r#"{{"jsonrpc":"2.0","result":{{"contents":[{{"uri":"file:///Users/demo/data/record{}.json","mimeType":"application/json","text":"{{\\"id\\":{},\\"status\\":\\"completed\\",\\"timestamp\\":1734720{}000}}"}}]}},"id":{}}}"#,
                    i,
                    i,
                    i,
                    33 + i
                ),
                None,
                Some(latency),
            );
        }

        // 21. A few more errors for variety
        add_log(16500, "in", r#"{"jsonrpc":"2.0","method":"tools/call","params":{"name":"unknown_tool","arguments":{}},"id":58}"#.to_string(), Some("tools/call"), None);
        add_log(16550, "out", r#"{"jsonrpc":"2.0","error":{"code":-32601,"message":"Method not found","data":{"method":"unknown_tool"}},"id":58}"#.to_string(), None, Some(50000));

        add_log(17000, "in", r#"{"jsonrpc":"2.0","method":"resources/read","params":{"uri":"postgres://localhost/db/table"},"id":59}"#.to_string(), Some("resources/read"), None);
        add_log(20000, "out", r#"{"jsonrpc":"2.0","error":{"code":-32603,"message":"Connection timeout","data":{"uri":"postgres://localhost/db/table","timeout":"3000ms"}},"id":59}"#.to_string(), None, Some(3000000));

        MockData { session, logs }
    }
}
