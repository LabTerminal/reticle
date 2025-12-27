import { useMemo } from 'react'
import { AreaChart, Area, XAxis, YAxis, Tooltip, ResponsiveContainer } from 'recharts'
import { Database, Trash2, Coins } from 'lucide-react'
import { toast } from 'sonner'
import { useReticleStore } from '@/store'
import { Button } from '@/components/ui/button'
import { ScrollArea } from '@/components/ui/scroll-area'
import { Logo } from '@/components/Logo'
import { cn } from '@/lib/utils'
import { format } from 'date-fns'
import { ServerFilter, TagFilter, QuickTagInput } from '@/components/SessionTags'

export function Sidebar() {
  const {
    sessions,
    currentSession,
    setCurrentSession,
    isConnected,
    logs,
    clearLogs,
    filters,
    setFilters,
  } = useReticleStore()

  // Calculate metrics from logs
  const metrics = useMemo(() => {
    const now = Date.now() * 1000 // Convert to microseconds
    const oneSecondAgo = now - 1_000_000
    const recentLogs = logs.filter((log) => log.timestamp > oneSecondAgo)

    // Messages per second over last 10 seconds
    const timeWindows = Array.from({ length: 10 }, (_, i) => {
      const windowStart = now - (10 - i) * 1_000_000
      const windowEnd = windowStart + 1_000_000
      const count = logs.filter(
        (log) => log.timestamp >= windowStart && log.timestamp < windowEnd
      ).length
      return {
        time: format(new Date(windowStart / 1000), 'HH:mm:ss'),
        mps: count,
      }
    })

    return {
      messagesPerSecond: recentLogs.length,
      totalMessages: logs.length,
      timeWindows,
    }
  }, [logs])

  // Get unique methods for filtering with counts
  const uniqueMethods = useMemo(() => {
    const methodCounts = new Map<string, number>()
    logs.forEach((log) => {
      if (log.method) {
        methodCounts.set(log.method, (methodCounts.get(log.method) || 0) + 1)
      }
    })
    return Array.from(methodCounts.entries())
      .map(([method, count]) => ({ method, count }))
      .sort((a, b) => a.method.localeCompare(b.method))
  }, [logs])

  // Calculate direction counts
  const directionCounts = useMemo(() => {
    const incoming = logs.filter(log => log.direction === 'in').length
    const outgoing = logs.filter(log => log.direction === 'out').length
    return { incoming, outgoing }
  }, [logs])

  // Calculate token statistics
  const tokenStats = useMemo(() => {
    let totalTokens = 0
    let tokensToServer = 0
    let tokensFromServer = 0
    const tokensByMethod: Record<string, number> = {}

    logs.forEach((log) => {
      const tokens = log.token_count || 0
      totalTokens += tokens

      if (log.direction === 'in') {
        tokensToServer += tokens
      } else {
        tokensFromServer += tokens
      }

      if (log.method) {
        tokensByMethod[log.method] = (tokensByMethod[log.method] || 0) + tokens
      }
    })

    // Sort methods by token count
    const topMethods = Object.entries(tokensByMethod)
      .sort((a, b) => b[1] - a[1])
      .slice(0, 5)

    return {
      totalTokens,
      tokensToServer,
      tokensFromServer,
      topMethods,
    }
  }, [logs])

  return (
    <div className="flex flex-col h-full bg-background border-r border-border overflow-hidden">
      {/* Header - Premium Branding */}
      <div className="glass-strong px-4 py-3 border-b border-border">
        <Logo variant="compact" />
      </div>

      <ScrollArea className="flex-1">
        <div className="p-4 space-y-6 overflow-x-hidden">
          {/* Connection Status - Premium LED Style */}
          <div>
            <h3 className="text-[10px] font-semibold mb-2 text-muted-foreground uppercase tracking-wider">
              Status
            </h3>
            <div
              className={cn(
                'px-3 py-2 rounded-md border text-[11px] font-mono font-medium',
                isConnected
                  ? 'bg-[#059669]/10 dark:bg-[#00FF9F]/10 border-[#059669]/30 dark:border-[#00FF9F]/30 text-[#059669] dark:text-[#00FF9F]'
                  : 'bg-[#DC2626]/10 dark:bg-[#FF003C]/10 border-[#DC2626]/30 dark:border-[#FF003C]/30 text-[#DC2626] dark:text-[#FF003C]'
              )}
            >
              <div className="flex items-center gap-2">
                <div
                  className={cn(
                    'w-1.5 h-1.5 rounded-full',
                    isConnected ? 'bg-[#059669] dark:bg-[#00FF9F] glow-success animate-pulse-glow' : 'bg-[#DC2626] dark:bg-[#FF003C] glow-error'
                  )}
                />
                {isConnected ? 'Connected' : 'Disconnected'}
              </div>
            </div>
          </div>

          {/* Metrics - Premium Cards */}
          <div>
            <h3 className="text-[10px] font-semibold mb-2 text-muted-foreground uppercase tracking-wider">
              Metrics
            </h3>
            <div className="space-y-2">
              <div className="flex items-center justify-between text-[11px]">
                <span className="text-muted-foreground">Total Messages</span>
                <span className="font-mono font-bold text-[#00808F] dark:text-[#00F0FF] tabular-nums">
                  {metrics.totalMessages}
                </span>
              </div>
              <div className="flex items-center justify-between text-[11px]">
                <span className="text-muted-foreground">MPS (1s)</span>
                <span className="font-mono font-bold text-[#059669] dark:text-[#00FF9F] tabular-nums">
                  {metrics.messagesPerSecond}
                </span>
              </div>
            </div>
          </div>

          {/* Token Profiling - Context Usage */}
          {tokenStats.totalTokens > 0 && (
            <div>
              <h3 className="text-[10px] font-semibold mb-2 text-muted-foreground uppercase tracking-wider flex items-center gap-1.5">
                <Coins className="w-3 h-3" />
                Token Usage
              </h3>
              <div className="space-y-2">
                <div className="flex items-center justify-between text-[11px]">
                  <span className="text-muted-foreground">Total Tokens</span>
                  <span className="font-mono font-bold text-[#F59E0B] tabular-nums">
                    {tokenStats.totalTokens.toLocaleString()}
                  </span>
                </div>
                <div className="flex items-center justify-between text-[11px]">
                  <span className="text-muted-foreground">→ To Server</span>
                  <span className="font-mono font-medium text-[#3B82F6] tabular-nums">
                    {tokenStats.tokensToServer.toLocaleString()}
                  </span>
                </div>
                <div className="flex items-center justify-between text-[11px]">
                  <span className="text-muted-foreground">← From Server</span>
                  <span className="font-mono font-medium text-[#10B981] tabular-nums">
                    {tokenStats.tokensFromServer.toLocaleString()}
                  </span>
                </div>

                {/* Top methods by tokens */}
                {tokenStats.topMethods.length > 0 && (
                  <div className="pt-2 border-t border-border/50">
                    <p className="text-[10px] text-muted-foreground mb-1.5">Top by tokens</p>
                    <div className="space-y-1">
                      {tokenStats.topMethods.map(([method, tokens]) => (
                        <div
                          key={method}
                          className="flex items-center justify-between text-[10px]"
                        >
                          <span className="font-mono text-muted-foreground truncate max-w-[120px]">
                            {method}
                          </span>
                          <span className="font-mono text-[#F59E0B] tabular-nums">
                            {tokens.toLocaleString()}
                          </span>
                        </div>
                      ))}
                    </div>
                  </div>
                )}
              </div>
            </div>
          )}

          {/* Messages Per Second Chart - Premium Style */}
          <div>
            <h3 className="text-[10px] font-semibold mb-3 text-muted-foreground uppercase tracking-wider">
              Activity (10s)
            </h3>
            <div className="h-32 bg-muted/40 border border-border rounded-md p-2">
              <ResponsiveContainer width="100%" height="100%">
                <AreaChart data={metrics.timeWindows}>
                  <defs>
                    <linearGradient id="colorMps" x1="0" y1="0" x2="0" y2="1">
                      <stop offset="5%" stopColor="#00F0FF" stopOpacity={0.8} />
                      <stop offset="95%" stopColor="#00F0FF" stopOpacity={0} />
                    </linearGradient>
                  </defs>
                  <XAxis
                    dataKey="time"
                    tick={{ fill: '#8B949E', fontSize: 9 }}
                    tickLine={false}
                    axisLine={false}
                  />
                  <YAxis
                    tick={{ fill: '#8B949E', fontSize: 9 }}
                    tickLine={false}
                    axisLine={false}
                    width={28}
                  />
                  <Tooltip
                    contentStyle={{
                      backgroundColor: '#0D1117',
                      border: '1px solid rgba(0,240,255,0.2)',
                      borderRadius: '6px',
                      fontSize: '11px',
                    }}
                  />
                  <Area
                    type="monotone"
                    dataKey="mps"
                    stroke="#00F0FF"
                    strokeWidth={2}
                    fillOpacity={1}
                    fill="url(#colorMps)"
                  />
                </AreaChart>
              </ResponsiveContainer>
            </div>
          </div>

          {/* Filters */}
          <div>
            <div className="flex items-center justify-between mb-2">
              <h3 className="text-xs font-semibold text-muted-foreground uppercase tracking-wider">
                Filters
              </h3>
              {(filters.method || filters.direction || filters.serverName || (filters.tags && filters.tags.length > 0)) && (
                <Button
                  variant="ghost"
                  size="sm"
                  onClick={() => useReticleStore.getState().clearFilters()}
                  className="h-8 px-3 text-xs"
                >
                  Clear
                </Button>
              )}
            </div>

            {/* Server Filter */}
            <div className="mb-3">
              <ServerFilter
                selectedServer={filters.serverName}
                onSelectServer={(server) => setFilters({ serverName: server })}
              />
            </div>

            {/* Tag Filter */}
            <div className="mb-3">
              <TagFilter
                selectedTags={filters.tags || []}
                onSelectTags={(tags) => setFilters({ tags })}
              />
            </div>

            {/* Direction Filter */}
            <div className="space-y-2 mb-3">
              <p className="text-xs text-muted-foreground">Direction</p>
              <div className="flex gap-2">
                <Button
                  variant={filters.direction === 'in' ? 'default' : 'outline'}
                  size="sm"
                  onClick={() =>
                    setFilters({
                      direction: filters.direction === 'in' ? undefined : 'in',
                    })
                  }
                  className="flex-1 h-9 text-xs flex items-center justify-between"
                >
                  <span>Incoming</span>
                  <span className="ml-1.5 px-1.5 py-0.5 rounded bg-muted text-[10px] font-mono tabular-nums">
                    {directionCounts.incoming}
                  </span>
                </Button>
                <Button
                  variant={filters.direction === 'out' ? 'default' : 'outline'}
                  size="sm"
                  onClick={() =>
                    setFilters({
                      direction: filters.direction === 'out' ? undefined : 'out',
                    })
                  }
                  className="flex-1 h-9 text-xs flex items-center justify-between"
                >
                  <span>Outgoing</span>
                  <span className="ml-1.5 px-1.5 py-0.5 rounded bg-muted text-[10px] font-mono tabular-nums">
                    {directionCounts.outgoing}
                  </span>
                </Button>
              </div>
            </div>

            {/* Method Filter */}
            {uniqueMethods.length > 0 && (
              <div className="space-y-2">
                <p className="text-xs text-muted-foreground">Methods</p>
                <ScrollArea className="h-40">
                  <div className="space-y-1">
                    {uniqueMethods.map(({ method, count }) => (
                      <button
                        key={method}
                        onClick={() =>
                          setFilters({
                            method: filters.method === method ? undefined : method,
                          })
                        }
                        className={cn(
                          'w-full flex items-center justify-between px-2 py-1.5 rounded text-xs font-mono transition-colors',
                          filters.method === method
                            ? 'bg-primary/20 text-primary border border-primary/50'
                            : 'hover:bg-muted/50 text-muted-foreground'
                        )}
                      >
                        <span className="truncate">{method}</span>
                        <span className={cn(
                          'ml-2 px-1.5 py-0.5 rounded text-[10px] font-mono tabular-nums flex-shrink-0',
                          filters.method === method
                            ? 'bg-primary/30 text-primary'
                            : 'bg-muted text-muted-foreground'
                        )}>
                          {count}
                        </span>
                      </button>
                    ))}
                  </div>
                </ScrollArea>
              </div>
            )}
          </div>

          {/* Sessions */}
          {sessions.length > 0 && (
            <div>
              <h3 className="text-xs font-semibold mb-2 text-muted-foreground uppercase tracking-wider">
                Sessions
              </h3>
              <ScrollArea className="h-40">
                <div className="space-y-1">
                  {sessions.map((session) => (
                    <button
                      key={session.id}
                      onClick={() => setCurrentSession(session.id)}
                      className={cn(
                        'w-full text-left px-2 py-2 rounded text-xs transition-colors',
                        currentSession?.id === session.id
                          ? 'bg-primary/20 text-primary border border-primary/50'
                          : 'hover:bg-muted/50 text-muted-foreground'
                      )}
                    >
                      <div className="flex items-center justify-between">
                        <Database className="w-3 h-3" />
                        <span className="text-[10px] font-mono">
                          {session.message_count} msgs
                        </span>
                      </div>
                      <div className="text-[10px] font-mono mt-1 truncate">
                        {session.id}
                      </div>
                      {/* Server name badge */}
                      {session.server_name && (
                        <div className="mt-1">
                          <span className="text-[9px] font-mono px-1 py-0.5 rounded bg-blue-500/20 text-blue-400">
                            {session.server_name}
                          </span>
                        </div>
                      )}
                      {/* Tags */}
                      {session.tags && session.tags.length > 0 && (
                        <div className="flex flex-wrap gap-1 mt-1">
                          {session.tags.map((tag) => (
                            <span
                              key={tag}
                              className="text-[9px] font-mono px-1 py-0.5 rounded bg-secondary text-secondary-foreground"
                            >
                              {tag}
                            </span>
                          ))}
                        </div>
                      )}
                    </button>
                  ))}
                </div>
              </ScrollArea>
            </div>
          )}

          {/* Quick Tag Input for Current Session */}
          {currentSession && (
            <QuickTagInput sessionId={currentSession.id} localOnly />
          )}

          {/* Actions */}
          <div>
            <h3 className="text-xs font-semibold mb-2 text-muted-foreground uppercase tracking-wider">
              Actions
            </h3>
            <Button
              variant="outline"
              size="sm"
              onClick={() => {
                clearLogs()
                toast.success('All logs cleared', {
                  duration: 2000,
                })
              }}
              disabled={logs.length === 0}
              className="w-full"
            >
              <Trash2 className="w-3 h-3 mr-2" />
              Clear Logs
            </Button>
          </div>
        </div>
      </ScrollArea>
    </div>
  )
}
