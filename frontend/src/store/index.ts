import { create } from 'zustand'
import { LogEntry, Session, FilterOptions, ParsedMessage } from '@/types'

const MAX_LOGS = 10000

interface ReticleStore {
  // Connection state
  isConnected: boolean
  setConnected: (connected: boolean) => void

  // Logs
  logs: LogEntry[]
  addLog: (log: LogEntry) => void
  clearLogs: () => void

  // Selected log for inspection
  selectedLogId: string | null
  selectLog: (id: string | null) => void

  // Sessions
  sessions: Session[]
  currentSession: Session | null
  addSession: (session: Session) => void
  setCurrentSession: (sessionId: string) => void
  updateSessionTags: (sessionId: string, tags: string[]) => void

  // Multi-server support
  availableServers: string[]
  availableTags: string[]
  setAvailableServers: (servers: string[]) => void
  setAvailableTags: (tags: string[]) => void

  // Filters
  filters: FilterOptions
  setFilters: (filters: Partial<FilterOptions>) => void
  clearFilters: () => void

  // Command palette
  isCommandOpen: boolean
  setCommandOpen: (open: boolean) => void

  // Computed getters
  getFilteredLogs: () => LogEntry[]
  getSelectedLog: () => LogEntry | null
}

export const useReticleStore = create<ReticleStore>((set, get) => ({
  // Connection state
  isConnected: false,
  setConnected: (connected) => set({ isConnected: connected }),

  // Logs
  logs: [],
  addLog: (log) =>
    set((state) => {
      // Check if log with this ID already exists (prevent duplicates)
      if (state.logs.some((existingLog) => existingLog.id === log.id)) {
        console.warn(`Duplicate log entry detected (same ID), skipping: ${log.id}`)
        return state
      }

      // Check for content-based duplicates within a short time window (500ms)
      // This handles cases where the same message is emitted from different sources
      // with different log IDs (e.g., sent request echoed back via stdout/SSE)
      const duplicateWindow = 500 // ms
      const recentLogs = state.logs.filter(
        (existingLog) =>
          Math.abs(existingLog.timestamp - log.timestamp) < duplicateWindow
      )
      if (recentLogs.some((existingLog) => existingLog.content === log.content)) {
        console.warn(`Duplicate log entry detected (same content within ${duplicateWindow}ms), skipping: ${log.id}`)
        return state
      }

      const newLogs = [...state.logs, log]
      // Keep only the most recent MAX_LOGS entries
      if (newLogs.length > MAX_LOGS) {
        return { logs: newLogs.slice(newLogs.length - MAX_LOGS) }
      }
      return { logs: newLogs }
    }),
  clearLogs: () => set({ logs: [], selectedLogId: null }),

  // Selected log
  selectedLogId: null,
  selectLog: (id) => set({ selectedLogId: id }),

  // Sessions
  sessions: [],
  currentSession: null,
  addSession: (session) =>
    set((state) => {
      // Check if session already exists to prevent duplicates
      const existingSession = state.sessions.find((s) => s.id === session.id)
      if (existingSession) {
        return { currentSession: existingSession }
      }
      return {
        sessions: [...state.sessions, session],
        currentSession: session,
      }
    }),
  setCurrentSession: (sessionId) =>
    set((state) => ({
      currentSession: state.sessions.find((s) => s.id === sessionId) || null,
    })),
  updateSessionTags: (sessionId, tags) =>
    set((state) => ({
      sessions: state.sessions.map((s) =>
        s.id === sessionId ? { ...s, tags } : s
      ),
      currentSession:
        state.currentSession?.id === sessionId
          ? { ...state.currentSession, tags }
          : state.currentSession,
    })),

  // Multi-server support
  availableServers: [],
  availableTags: [],
  setAvailableServers: (servers) => set({ availableServers: servers }),
  setAvailableTags: (tags) => set({ availableTags: tags }),

  // Filters
  filters: {},
  setFilters: (filters) =>
    set((state) => ({
      filters: { ...state.filters, ...filters },
    })),
  clearFilters: () => set({ filters: {} }),

  // Command palette
  isCommandOpen: false,
  setCommandOpen: (open) => set({ isCommandOpen: open }),

  // Computed getters
  getFilteredLogs: () => {
    const { logs, filters } = get()

    return logs.filter((log) => {
      // Filter by session
      if (filters.sessionId && log.session_id !== filters.sessionId) {
        return false
      }

      // Filter by direction
      if (filters.direction && log.direction !== filters.direction) {
        return false
      }

      // Filter by method
      if (filters.method && log.method !== filters.method) {
        return false
      }

      // Filter by server name
      if (filters.serverName && log.server_name !== filters.serverName) {
        return false
      }

      // Filter by search text
      if (filters.searchText) {
        const searchLower = filters.searchText.toLowerCase()
        const contentMatch = log.content.toLowerCase().includes(searchLower)
        const methodMatch = log.method?.toLowerCase().includes(searchLower)
        if (!contentMatch && !methodMatch) {
          return false
        }
      }

      // Filter by minimum latency (for responses with duration)
      if (filters.minLatencyMs !== undefined && filters.minLatencyMs > 0) {
        const minLatencyMicros = filters.minLatencyMs * 1000
        // Only include logs that have duration and meet the threshold
        if (!log.duration_micros || log.duration_micros < minLatencyMicros) {
          return false
        }
      }

      return true
    })
  },

  getSelectedLog: () => {
    const { logs, selectedLogId } = get()
    if (!selectedLogId) return null
    return logs.find((log) => log.id === selectedLogId) || null
  },
}))

/**
 * Parse JSON-RPC message from log entry
 */
export function parseLogMessage(log: LogEntry): ParsedMessage | null {
  try {
    return JSON.parse(log.content) as ParsedMessage
  } catch {
    return null
  }
}

/**
 * Extract method name from log entry
 */
export function extractMethod(log: LogEntry): string | undefined {
  const parsed = parseLogMessage(log)
  return parsed?.method
}

/**
 * Find the corresponding request for a response
 */
export function findCorrelatedRequest(
  response: LogEntry,
  allLogs: LogEntry[]
): LogEntry | null {
  const parsedResponse = parseLogMessage(response)
  if (!parsedResponse?.id || parsedResponse.method) {
    // Not a response (has method) or no id
    return null
  }

  // Find the request with matching id that came before this response
  const responseIdx = allLogs.findIndex((log) => log.id === response.id)
  if (responseIdx === -1) return null

  // Search backwards for matching request
  for (let i = responseIdx - 1; i >= 0; i--) {
    const log = allLogs[i]
    if (log.direction === 'in') {
      const parsed = parseLogMessage(log)
      if (parsed?.id === parsedResponse.id && parsed.method) {
        return log
      }
    }
  }

  return null
}

/**
 * Calculate latency between request and response
 */
export function calculateLatency(
  request: LogEntry,
  response: LogEntry
): number {
  return response.timestamp - request.timestamp
}

// Legacy alias for backwards compatibility
export const useSentinelStore = useReticleStore
