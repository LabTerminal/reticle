import { useEffect, useRef, memo, useState } from 'react'
import { Virtuoso, VirtuosoHandle } from 'react-virtuoso'
import {
  ArrowDownCircle,
  ArrowUpCircle,
  Filter,
  Zap,
  Search,
  X,
  Pause,
  Play,
  Copy,
} from 'lucide-react'
import { toast } from 'sonner'
import {
  useReticleStore,
  parseLogMessage,
  findCorrelatedRequest,
  calculateLatency,
} from '@/store'
import { LogEntry, ParsedMessage } from '@/types'
import { cn, formatTimestamp, truncate, formatDuration } from '@/lib/utils'
import { Input } from '@/components/ui/input'
import { Button } from '@/components/ui/button'

/**
 * Individual log row component - heavily memoized for performance
 * Premium Design: 32px height, status dots, method badges, copy on hover
 */
const LogRow = memo(({ log }: { log: LogEntry; index: number }) => {
  const { selectedLogId, selectLog, logs } = useReticleStore()
  const [isHovered, setIsHovered] = useState(false)
  const isSelected = selectedLogId === log.id
  const rowRef = useRef<HTMLDivElement>(null)

  // Check if this is a raw/stderr message (non-JSON-RPC)
  const isRawMessage = log.message_type === 'raw'
  const isStderrMessage = log.message_type === 'stderr'
  const isNonJsonRpc = isRawMessage || isStderrMessage

  const parsed = isNonJsonRpc ? null : parseLogMessage(log)
  const isError = parsed?.error !== undefined || isStderrMessage
  const isRequest = parsed?.method !== undefined && !parsed.result && !parsed.error
  const isResponse = (parsed?.result !== undefined || parsed?.error !== undefined) && !parsed.method

  // Find correlated request for responses
  const correlatedRequest = isResponse ? findCorrelatedRequest(log, logs) : null
  const actualLatency = correlatedRequest ? calculateLatency(correlatedRequest, log) : null

  // Get method name or result summary
  const method = isStderrMessage
    ? 'stderr'
    : isRawMessage
    ? 'raw'
    : parsed?.method || (correlatedRequest ? parseLogMessage(correlatedRequest)?.method || 'response' : 'response')
  const summary = isNonJsonRpc ? log.content : getSummary(parsed)

  // Get JSON-RPC id for correlation display
  const rpcId = parsed?.id

  // Copy JSON to clipboard
  const handleCopyJson = (e: React.MouseEvent) => {
    e.stopPropagation()
    navigator.clipboard.writeText(log.content)
    toast.success('JSON copied to clipboard', {
      duration: 2000,
    })
  }

  // Status color mapping (Light: WCAG-compliant colors, Dark: Neon colors)
  const getStatusColor = () => {
    if (isStderrMessage) return 'bg-[#DC2626] dark:bg-[#FF003C]' // Stderr is always red
    if (isRawMessage) return 'bg-[#D97706] dark:bg-[#FCEE09]' // Raw output is warning-colored
    if (isError) return 'bg-[#DC2626] dark:bg-[#FF003C]'
    if (isRequest) return 'bg-[#00808F] dark:bg-[#00F0FF]'
    return 'bg-[#059669] dark:bg-[#00FF9F]'
  }

  // Latency color coding (Light: WCAG-compliant, Dark: Neon)
  const getLatencyColor = (micros: number) => {
    if (micros > 1000000) return 'text-[#DC2626] dark:text-[#FF003C]' // >1s: Red
    if (micros > 200000) return 'text-[#D97706] dark:text-[#FCEE09]' // >200ms: Yellow/Orange
    if (micros > 50000) return 'text-muted-foreground' // >50ms: Gray
    return 'text-muted-foreground/70' // <50ms: Dim gray
  }

  // Scroll into view when selected
  useEffect(() => {
    if (isSelected && rowRef.current) {
      rowRef.current.scrollIntoView({ block: 'nearest', behavior: 'smooth' })
    }
  }, [isSelected])

  return (
    <div
      ref={rowRef}
      tabIndex={isSelected ? 0 : -1}
      role="button"
      aria-selected={isSelected}
      onClick={() => selectLog(log.id)}
      onMouseEnter={() => setIsHovered(true)}
      onMouseLeave={() => setIsHovered(false)}
      className={cn(
        // Base: Compact 32px height, premium hover states
        'group relative flex items-center gap-3 px-4 h-8 cursor-pointer border-b border-border transition-all duration-150 log-row-enter',
        // Hover: Subtle background
        'hover:bg-muted/40',
        // Selected: Left border accent (Light: Optic Azure, Dark: Reticle Cyan)
        isSelected && 'border-l-2 border-l-[#00808F] dark:border-l-[#00F0FF] bg-muted/60',
        // Error: Red accent (Light: Fatal Crimson, Dark: Critical Red)
        isError && !isSelected && 'border-l-2 border-l-[#DC2626] dark:border-l-[#FF003C] bg-[#DC2626]/5 dark:bg-[#FF003C]/5',
        // Default: Transparent border
        !isSelected && !isError && 'border-l-2 border-l-transparent',
        // Focus visible ring (Light: Optic Azure, Dark: Reticle Cyan)
        'focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[#00808F] dark:focus-visible:ring-[#00F0FF] focus-visible:ring-offset-2 focus-visible:ring-offset-background'
      )}
    >
      {/* Status Dot (Premium LED indicator) */}
      <div className="flex-shrink-0">
        <div
          className={cn(
            'w-2 h-2 rounded-full transition-all',
            getStatusColor(),
            isError && 'glow-error',
            isSelected && 'glow-primary'
          )}
        />
      </div>

      {/* Timestamp */}
      <span className="text-xs text-muted-foreground font-mono w-20 flex-shrink-0 tabular-nums">
        {formatTimestamp(log.timestamp)}
      </span>

      {/* Direction Icon (minimal) */}
      <div className="flex-shrink-0">
        {log.direction === 'in' ? (
          <ArrowDownCircle className="w-3 h-3 text-muted-foreground" />
        ) : (
          <ArrowUpCircle className="w-3 h-3 text-muted-foreground" />
        )}
      </div>

      {/* Method Badge (Premium styling) */}
      <span
        className={cn(
          'inline-flex items-center px-1.5 py-0.5 rounded-md text-xs font-mono font-medium flex-shrink-0 min-w-[140px]',
          isStderrMessage
            ? 'bg-[#DC2626]/20 dark:bg-[#FF003C]/20 text-[#DC2626] dark:text-[#FF003C] border border-[#DC2626]/30 dark:border-[#FF003C]/30'
            : isRawMessage
            ? 'bg-[#D97706]/20 dark:bg-[#FCEE09]/20 text-[#D97706] dark:text-[#FCEE09] border border-[#D97706]/30 dark:border-[#FCEE09]/30'
            : 'bg-secondary text-secondary-foreground border border-border'
        )}
      >
        {method}
        {rpcId !== undefined && (
          <span className="text-muted-foreground ml-1.5">#{rpcId}</span>
        )}
      </span>

      {/* Summary (truncated) */}
      <span className="text-xs text-muted-foreground flex-1 truncate font-mono">
        {summary}
      </span>

      {/* Token count */}
      {log.token_count !== undefined && log.token_count > 0 && (
        <span
          className="text-[10px] font-mono flex-shrink-0 px-1.5 py-0.5 rounded bg-[#F59E0B]/10 text-[#F59E0B] tabular-nums"
          title={`Estimated ${log.token_count.toLocaleString()} tokens`}
        >
          {log.token_count >= 1000
            ? `${(log.token_count / 1000).toFixed(1)}k`
            : log.token_count}
        </span>
      )}

      {/* Latency (Premium color scale) */}
      {(actualLatency !== null || log.duration_micros !== undefined) && (
        <span
          className={cn(
            'text-xs font-mono flex-shrink-0 w-16 text-right tabular-nums',
            getLatencyColor(actualLatency || log.duration_micros || 0)
          )}
          title={
            actualLatency !== null
              ? `Round-trip latency from request #${parsed?.id}`
              : undefined
          }
        >
          {formatDuration(actualLatency || log.duration_micros || 0)}
        </span>
      )}

      {/* Copy JSON Button (Instant on hover) */}
      {isHovered && (
        <Button
          variant="ghost"
          size="sm"
          onClick={handleCopyJson}
          className="absolute right-2 h-7 w-7 p-0 bg-secondary border border-border hover:bg-muted"
          title="Copy JSON"
        >
          <Copy className="w-3.5 h-3.5 text-muted-foreground" />
        </Button>
      )}
    </div>
  )
})
LogRow.displayName = 'LogRow'

/**
 * Get a human-readable summary of the log message
 */
function getSummary(parsed: ParsedMessage | null): string {
  if (!parsed) return 'Invalid JSON'

  if (parsed.error) {
    return `Error ${parsed.error.code}: ${truncate(parsed.error.message, 60)}`
  }

  if (parsed.params) {
    const paramStr = JSON.stringify(parsed.params)
    return truncate(paramStr, 80)
  }

  if (parsed.result) {
    const resultStr = JSON.stringify(parsed.result)
    return truncate(resultStr, 80)
  }

  return 'No content'
}

/**
 * Main LogStream component with virtualization
 */
export function LogStream() {
  const filteredLogs = useReticleStore((state) => state.getFilteredLogs())
  const allLogs = useReticleStore((state) => state.logs)
  const { filters, setFilters, selectedLogId, selectLog } = useReticleStore()
  const virtuosoRef = useRef<VirtuosoHandle>(null)
  const isAutoScrollEnabled = useRef(true)
  const [isLive, setIsLive] = useState(true)
  const searchInputRef = useRef<HTMLInputElement>(null)
  const [searchValue, setSearchValue] = useState(filters.searchText || '')

  // Check if filters are active
  const hasActiveFilters = !!(filters.searchText || filters.method || filters.direction)

  // Auto-scroll to bottom when new logs arrive
  useEffect(() => {
    if (isAutoScrollEnabled.current && filteredLogs.length > 0) {
      virtuosoRef.current?.scrollToIndex({
        index: filteredLogs.length - 1,
        behavior: 'smooth',
      })
    }
  }, [filteredLogs.length])

  // Keyboard shortcut for search (Cmd+K / Ctrl+K) and arrow key navigation
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      // Don't interfere with search input or other inputs
      if (document.activeElement instanceof HTMLInputElement ||
          document.activeElement instanceof HTMLTextAreaElement) {
        // Allow Cmd+K to focus search even when in input
        if ((e.metaKey || e.ctrlKey) && e.key === 'k') {
          e.preventDefault()
          searchInputRef.current?.focus()
        }
        return
      }

      // Cmd+K: Focus search
      if ((e.metaKey || e.ctrlKey) && e.key === 'k') {
        e.preventDefault()
        searchInputRef.current?.focus()
        return
      }

      // Arrow key navigation
      if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
        e.preventDefault()

        const currentIndex = selectedLogId
          ? filteredLogs.findIndex(log => log.id === selectedLogId)
          : -1

        if (e.key === 'ArrowDown') {
          // Move down
          if (currentIndex < filteredLogs.length - 1) {
            const nextLog = filteredLogs[currentIndex + 1]
            if (nextLog) {
              selectLog(nextLog.id)
            }
          }
        } else if (e.key === 'ArrowUp') {
          // Move up
          if (currentIndex > 0) {
            const prevLog = filteredLogs[currentIndex - 1]
            if (prevLog) {
              selectLog(prevLog.id)
            }
          } else if (currentIndex === -1 && filteredLogs.length > 0) {
            // If nothing selected, select first item
            selectLog(filteredLogs[0].id)
          }
        }
        return
      }

      // Escape: Clear selection
      if (e.key === 'Escape') {
        e.preventDefault()
        selectLog(null)
        return
      }

      // Enter: Toggle selection (if something is already selected, deselect)
      if (e.key === 'Enter' && selectedLogId) {
        e.preventDefault()
        selectLog(null)
        return
      }
    }

    window.addEventListener('keydown', handleKeyDown)
    return () => window.removeEventListener('keydown', handleKeyDown)
  }, [filteredLogs, selectedLogId, selectLog])

  // Update search filter with debounce
  useEffect(() => {
    const timer = setTimeout(() => {
      setFilters({ searchText: searchValue || undefined })
    }, 300)
    return () => clearTimeout(timer)
  }, [searchValue, setFilters])

  const handleClearSearch = () => {
    setSearchValue('')
    setFilters({ searchText: undefined })
  }

  const toggleAutoScroll = () => {
    const newState = !isLive
    setIsLive(newState)
    isAutoScrollEnabled.current = newState

    // If re-enabling, scroll to bottom immediately
    if (newState && filteredLogs.length > 0) {
      virtuosoRef.current?.scrollToIndex({
        index: filteredLogs.length - 1,
        behavior: 'smooth',
      })
    }
  }

  return (
    <div className="flex flex-col h-full bg-background">
      {/* Header - Premium Glass Panel */}
      <div className="glass-strong flex items-center justify-between px-4 py-2.5 border-b border-border">
        <div className="flex items-center gap-3">
          <h2 className="text-sm font-semibold text-foreground">Message Stream</h2>
          <span className="text-xs text-muted-foreground bg-muted px-2 py-0.5 rounded-md border border-border">
            {filteredLogs.length}
          </span>

          {/* Live Indicator - LED Style */}
          {isLive && (
            <div className="flex items-center gap-1.5 px-2 py-0.5 bg-[#059669]/10 dark:bg-[#00FF9F]/10 border border-[#059669]/30 dark:border-[#00FF9F]/30 rounded-md">
              <div className="w-1.5 h-1.5 rounded-full bg-[#059669] dark:bg-[#00FF9F] glow-success animate-pulse-glow" />
              <span className="text-xs font-semibold text-[#059669] dark:text-[#00FF9F] tracking-wide">LIVE</span>
            </div>
          )}
        </div>

        <div className="flex items-center gap-2">
          {/* Auto-scroll Toggle */}
          <Button
            variant="ghost"
            size="sm"
            onClick={toggleAutoScroll}
            className={cn(
              'h-9 px-3 border border-border',
              isLive ? 'text-[#059669] dark:text-[#00FF9F] hover:bg-[#059669]/10 dark:hover:bg-[#00FF9F]/10' : 'text-muted-foreground hover:bg-muted'
            )}
            title={isLive ? 'Pause auto-scroll' : 'Resume auto-scroll'}
          >
            {isLive ? <Pause className="w-4 h-4" /> : <Play className="w-4 h-4" />}
          </Button>

          {/* Search Input - Premium Styling */}
          <div className="relative w-72">
            <Search className="absolute left-2.5 top-1/2 -translate-y-1/2 w-3.5 h-3.5 text-muted-foreground" />
            <Input
              ref={searchInputRef}
              type="text"
              placeholder="Search messages... (⌘K)"
              value={searchValue}
              onChange={(e) => setSearchValue(e.target.value)}
              className="pl-8 pr-10 h-9 text-xs bg-background border-border focus:border-[#00808F]/50 dark:focus:border-[#00F0FF]/50 focus:ring-1 focus:ring-[#00808F]/30 dark:focus:ring-[#00F0FF]/30"
            />
            {searchValue && (
              <Button
                variant="ghost"
                size="sm"
                onClick={handleClearSearch}
                className="absolute right-0.5 top-1/2 -translate-y-1/2 h-8 w-8 p-0 hover:bg-muted"
              >
                <X className="w-3.5 h-3.5 text-muted-foreground" />
              </Button>
            )}
          </div>
        </div>
      </div>

      {/* Virtualized List */}
      <div className="flex-1 overflow-hidden">
        {filteredLogs.length === 0 ? (
          <div className="flex items-center justify-center h-full">
            <div className="text-center max-w-sm">
              {hasActiveFilters && allLogs.length > 0 ? (
                <>
                  {/* Filtered out - no matches */}
                  <Filter className="w-10 h-10 mx-auto mb-3 text-[#D97706] dark:text-[#FCEE09]" />
                  <p className="text-sm text-foreground font-medium">No matches found</p>
                  <p className="text-xs text-muted-foreground mt-2">
                    {allLogs.length} message{allLogs.length === 1 ? '' : 's'} hidden by filters.
                  </p>
                  <Button
                    variant="outline"
                    size="sm"
                    onClick={() => {
                      setFilters({ searchText: undefined, method: undefined, direction: undefined })
                      setSearchValue('')
                    }}
                    className="mt-4 h-8 text-xs"
                  >
                    Clear all filters
                  </Button>
                </>
              ) : (
                <>
                  {/* No data yet - helpful getting started message */}
                  <div className="w-16 h-16 mx-auto mb-4 rounded-2xl bg-gradient-to-br from-[#00808F]/20 to-[#00808F]/5 dark:from-[#00F0FF]/20 dark:to-[#00F0FF]/5 flex items-center justify-center">
                    <Zap className="w-8 h-8 text-[#00808F] dark:text-[#00F0FF]" />
                  </div>
                  <p className="text-base text-foreground font-semibold mb-2">Ready to inspect MCP traffic</p>
                  <p className="text-sm text-muted-foreground mb-4 max-w-xs mx-auto">
                    Start a proxy to capture messages between your client and MCP server.
                  </p>
                  <div className="text-xs text-muted-foreground/80 space-y-1.5">
                    <p><span className="font-medium text-foreground">Demo</span> - Try with sample data</p>
                    <p><span className="font-medium text-foreground">Stdio</span> - Local MCP servers</p>
                    <p><span className="font-medium text-foreground">Remote</span> - HTTP/WebSocket servers</p>
                  </div>
                  <p className="text-[11px] text-muted-foreground/60 mt-4">
                    Press <kbd className="px-1.5 py-0.5 bg-muted border border-border rounded text-[10px] font-mono">?</kbd> for keyboard shortcuts
                  </p>
                </>
              )}
            </div>
          </div>
        ) : (
          <Virtuoso
            ref={virtuosoRef}
            data={filteredLogs}
            itemContent={(index, log) => <LogRow key={log.id} log={log} index={index} />}
            followOutput={(isAtBottom) => {
              // Update live state when user scrolls
              if (!isAtBottom && isLive) {
                setIsLive(false)
                isAutoScrollEnabled.current = false
              }
              return isAutoScrollEnabled.current && isAtBottom ? 'smooth' : false
            }}
            className="scrollbar-thin"
            increaseViewportBy={200}
            overscan={10}
          />
        )}
      </div>
    </div>
  )
}
