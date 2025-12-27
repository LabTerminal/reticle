/**
 * Core types for Reticle frontend
 */

export type Direction = 'in' | 'out'

/** Type of message content */
export type MessageType = 'jsonrpc' | 'raw' | 'stderr'

export interface LogEntry {
  id: string
  session_id: string
  timestamp: number // microseconds since epoch
  direction: Direction
  content: string // Raw JSON-RPC message or raw text
  method?: string // Extracted method name for quick filtering
  duration_micros?: number // For responses, time since request
  message_type?: MessageType // Type of content (jsonrpc, raw, stderr)
  token_count?: number // Estimated token count for this message
  server_name?: string // Server name for multi-server filtering
}

export interface ParsedMessage {
  jsonrpc: string
  id?: string | number
  method?: string
  params?: unknown
  result?: unknown
  error?: {
    code: number
    message: string
    data?: unknown
  }
}

export interface Session {
  id: string
  name?: string // User-friendly session name/alias
  started_at: number
  message_count: number
  last_activity: number
  server_name?: string // Server name for multi-server support
  tags?: string[] // Custom tags for filtering
}

export interface MetricsData {
  timestamp: number
  messages_per_second: number
  avg_latency_micros: number
}

export interface FilterOptions {
  method?: string
  direction?: Direction
  searchText?: string
  sessionId?: string
  serverName?: string // Filter by server name
  tags?: string[] // Filter by tags
  minLatencyMs?: number // Filter by minimum latency (50, 200, 1000 ms)
}

/** Token statistics per method */
export interface MethodTokenStats {
  total_tokens: number
  request_tokens: number
  response_tokens: number
  call_count: number
}

/** Token statistics for a session */
export interface SessionTokenStats {
  session_id: string
  tokens_to_server: number
  tokens_from_server: number
  total_tokens: number
  tokens_by_method: Record<string, MethodTokenStats>
  tool_definitions_tokens: number
  tool_count: number
  prompt_definitions_tokens: number
  prompt_count: number
  resource_definitions_tokens: number
  resource_count: number
}

/** Global token statistics */
export interface GlobalTokenStats {
  total_tokens: number
  sessions: Record<string, SessionTokenStats>
}

/** Individual tool token information */
export interface ToolTokenInfo {
  name: string
  description: string
  name_tokens: number
  description_tokens: number
  schema_tokens: number
  total_tokens: number
}

/** Tools analysis */
export interface ToolsAnalysis {
  count: number
  total_tokens: number
  tools: ToolTokenInfo[]
}

/** Individual prompt token information */
export interface PromptTokenInfo {
  name: string
  description?: string
  total_tokens: number
}

/** Prompts analysis */
export interface PromptsAnalysis {
  count: number
  total_tokens: number
  prompts: PromptTokenInfo[]
}

/** Individual resource token information */
export interface ResourceTokenInfo {
  uri: string
  name: string
  description?: string
  total_tokens: number
}

/** Resources analysis */
export interface ResourcesAnalysis {
  count: number
  total_tokens: number
  resources: ResourceTokenInfo[]
}

/** MCP Server analysis result */
export interface ServerAnalysis {
  server_name: string
  server_version: string
  protocol_version: string
  total_context_tokens: number
  tools: ToolsAnalysis
  prompts: PromptsAnalysis
  resources: ResourcesAnalysis
  token_breakdown: Record<string, number>
  analyzed_at: number
}

/** Session info from storage (for listing) */
export interface SessionInfo {
  id: string
  name: string
  started_at: number
  ended_at?: number
  message_count: number
  duration_ms?: number
  transport: string
  server_name?: string
  tags: string[]
}

/** Filter for querying sessions */
export interface SessionFilter {
  server_name?: string
  tags?: string[]
  transport?: string
}

/** Session metadata response from backend */
export interface SessionMetadata {
  id: string
  name: string
  started_at: number
  ended_at?: number
  transport: string
  server_name?: string
  server_version?: string
  server_command?: string
  connection_type?: string
  tags: string[]
  message_count: number
  duration_ms?: number
}
