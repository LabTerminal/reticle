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
  started_at: number
  message_count: number
  last_activity: number
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
