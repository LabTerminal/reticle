import { useState } from 'react'
import { Play, Pause, Settings, Loader2 } from 'lucide-react'
import { toast } from 'sonner'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { useReticleStore } from '@/store'
import { invoke } from '@tauri-apps/api/core'

// Transport type definitions matching Rust backend
type TransportType = 'stdio' | 'http' | 'streamable'

interface StdioConfig {
  type: 'stdio'
  command: string
  args: string[]
}

interface HttpConfig {
  type: 'http'
  server_url: string
  proxy_port: number
}

interface StreamableConfig {
  type: 'streamable'
  server_url: string
  proxy_port: number
}

type TransportConfig = StdioConfig | HttpConfig | StreamableConfig

export function ProxyControls() {
  const [isRunning, setIsRunning] = useState(false)
  const [isLoading, setIsLoading] = useState(false)
  const [showConfig, setShowConfig] = useState(false)
  const [transportType, setTransportType] = useState<TransportType>('stdio')

  // Stdio config
  const [command, setCommand] = useState('python3')
  const [args, setArgs] = useState('scripts/mock-mcp-server.py')

  // HTTP config (shared by legacy SSE and streamable transports)
  const [serverUrl, setServerUrl] = useState('http://localhost:8080')
  const [proxyPort, setProxyPort] = useState(3001)

  const { clearLogs } = useReticleStore()

  const startProxy = async () => {
    setIsLoading(true)
    try {
      let transportConfig: TransportConfig

      if (transportType === 'stdio') {
        const argsList = args.split(' ').filter(a => a.trim())
        transportConfig = {
          type: 'stdio',
          command: command.trim(),
          args: argsList,
        }
      } else if (transportType === 'http') {
        transportConfig = {
          type: 'http',
          server_url: serverUrl.trim(),
          proxy_port: proxyPort,
        }
      } else {
        // streamable transport
        transportConfig = {
          type: 'streamable',
          server_url: serverUrl.trim(),
          proxy_port: proxyPort,
        }
      }

      const result = await invoke('start_proxy_v2', {
        transportConfig,
      })

      setIsRunning(true)
      setShowConfig(false)

      toast.success('Proxy started', {
        description: typeof result === 'string' ? result : `${transportType.toUpperCase()} proxy mode active`,
        duration: 3000,
      })
    } catch (error) {
      console.error('Failed to start proxy:', error)
      toast.error('Failed to start proxy', {
        description: error instanceof Error ? error.message : 'Unknown error occurred',
        duration: 4000,
      })
    } finally {
      setIsLoading(false)
    }
  }

  const stopProxy = async () => {
    setIsLoading(true)
    try {
      await invoke('stop_proxy')
      setIsRunning(false)
      toast.success('Proxy stopped', {
        duration: 2000,
      })
    } catch (error) {
      console.error('Failed to stop proxy:', error)
      toast.error('Failed to stop proxy', {
        description: error instanceof Error ? error.message : 'Unknown error occurred',
        duration: 4000,
      })
    } finally {
      setIsLoading(false)
    }
  }

  const resetLogs = () => {
    clearLogs()
    toast.success('Logs cleared', { duration: 1000 })
  }

  return (
    <div className="glass-strong border-b border-white/5">
      <div className="flex items-center gap-2 px-4 py-2.5">
        <div className="flex items-center gap-2">
          <span className="text-[11px] font-semibold text-zinc-500 uppercase tracking-wider">
            Real Proxy
          </span>

          {!showConfig && !isRunning && (
            <Button
              variant="default"
              size="sm"
              onClick={() => setShowConfig(true)}
              className="h-9 px-4 bg-emerald-500/20 hover:bg-emerald-500/30 text-emerald-400 border border-emerald-500/50 text-xs font-medium"
            >
              <Settings className="w-3.5 h-3.5 mr-2" />
              Configure
            </Button>
          )}

          {!isRunning && showConfig && (
            <div className="flex items-center gap-2">
              {/* Transport type selector */}
              <div className="flex items-center gap-1 px-2 py-1 rounded-md bg-zinc-900/50 border border-white/10">
                <button
                  onClick={() => setTransportType('stdio')}
                  className={`px-3 py-1 rounded text-xs font-medium transition-colors ${
                    transportType === 'stdio'
                      ? 'bg-emerald-500/20 text-emerald-400'
                      : 'text-zinc-400 hover:text-zinc-300'
                  }`}
                >
                  stdio
                </button>
                <button
                  onClick={() => setTransportType('streamable')}
                  className={`px-3 py-1 rounded text-xs font-medium transition-colors ${
                    transportType === 'streamable'
                      ? 'bg-emerald-500/20 text-emerald-400'
                      : 'text-zinc-400 hover:text-zinc-300'
                  }`}
                  title="Streamable HTTP Transport (MCP 2025-03-26)"
                >
                  Streamable
                </button>
                <button
                  onClick={() => setTransportType('http')}
                  className={`px-3 py-1 rounded text-xs font-medium transition-colors ${
                    transportType === 'http'
                      ? 'bg-emerald-500/20 text-emerald-400'
                      : 'text-zinc-400 hover:text-zinc-300'
                  }`}
                  title="Legacy HTTP+SSE Transport (MCP 2024-11-05)"
                >
                  SSE (Legacy)
                </button>
              </div>

              {/* Stdio configuration */}
              {transportType === 'stdio' && (
                <>
                  <Input
                    placeholder="Command (e.g., python3)"
                    value={command}
                    onChange={(e) => setCommand(e.target.value)}
                    className="h-9 w-40 bg-zinc-900/50 border-white/10 text-xs"
                  />
                  <Input
                    placeholder="Args (e.g., server.py --port 3000)"
                    value={args}
                    onChange={(e) => setArgs(e.target.value)}
                    className="h-9 w-64 bg-zinc-900/50 border-white/10 text-xs"
                  />
                </>
              )}

              {/* HTTP configuration (streamable and legacy SSE) */}
              {(transportType === 'http' || transportType === 'streamable') && (
                <>
                  <Input
                    placeholder="Server URL (e.g., http://localhost:8080)"
                    value={serverUrl}
                    onChange={(e) => setServerUrl(e.target.value)}
                    className="h-9 w-64 bg-zinc-900/50 border-white/10 text-xs"
                  />
                  <Input
                    type="number"
                    placeholder="Proxy Port (e.g., 3001)"
                    value={proxyPort}
                    onChange={(e) => setProxyPort(parseInt(e.target.value) || 3001)}
                    className="h-9 w-32 bg-zinc-900/50 border-white/10 text-xs"
                  />
                </>
              )}

              <Button
                variant="default"
                size="sm"
                onClick={startProxy}
                disabled={
                  isLoading ||
                  (transportType === 'stdio' && !command.trim()) ||
                  ((transportType === 'http' || transportType === 'streamable') && !serverUrl.trim())
                }
                className="h-9 px-4 bg-emerald-500/20 hover:bg-emerald-500/30 text-emerald-400 border border-emerald-500/50 text-xs font-medium"
              >
                {isLoading ? (
                  <Loader2 className="w-3.5 h-3.5 mr-2 animate-spin" />
                ) : (
                  <Play className="w-3.5 h-3.5 mr-2" />
                )}
                {isLoading ? 'Starting...' : 'Start'}
              </Button>
              <Button
                variant="ghost"
                size="sm"
                onClick={() => setShowConfig(false)}
                className="h-9 px-3 hover:bg-zinc-800 text-zinc-400 text-xs"
              >
                Cancel
              </Button>
            </div>
          )}

          {isRunning && (
            <>
              <div className="flex items-center gap-2 px-3 py-1.5 rounded-md bg-emerald-500/10 border border-emerald-500/30">
                <span className="animate-pulse w-1.5 h-1.5 bg-emerald-400 rounded-full glow-success" />
                <span className="text-[11px] font-medium text-emerald-400">
                  Active
                </span>
              </div>
              <Button
                variant="outline"
                size="sm"
                onClick={stopProxy}
                disabled={isLoading}
                className="h-9 px-4 border-white/10 hover:bg-zinc-800 text-xs text-zinc-300"
              >
                {isLoading ? (
                  <Loader2 className="w-3.5 h-3.5 mr-2 animate-spin" />
                ) : (
                  <Pause className="w-3.5 h-3.5 mr-2" />
                )}
                {isLoading ? 'Stopping...' : 'Stop'}
              </Button>
            </>
          )}

          <Button
            variant="ghost"
            size="sm"
            onClick={resetLogs}
            disabled={isRunning}
            className="h-9 px-3 hover:bg-zinc-800 text-zinc-400 text-xs"
            title="Clear all logs"
          >
            Clear Logs
          </Button>
        </div>

        <div className="flex-1" />

        {isRunning && (
          <div className="text-[11px] text-zinc-400">
            <span className="font-mono">
              {transportType === 'stdio'
                ? `${command} ${args}`
                : transportType === 'streamable'
                ? `Streamable HTTP: ${serverUrl} → :${proxyPort}`
                : `HTTP/SSE Proxy: ${serverUrl} → :${proxyPort}`
              }
            </span>
          </div>
        )}
      </div>

      {!isRunning && !showConfig && (
        <div className="px-4 pb-3 pt-2 border-t border-white/5 bg-zinc-950/50">
          <div className="flex items-start gap-3">
            <div className="flex-shrink-0 w-1 h-20 bg-emerald-500/30 rounded-full" />
            <div className="flex-1">
              <p className="text-xs text-zinc-400 leading-relaxed">
                <span className="font-medium text-zinc-300">Debug MCP Servers:</span> Click Configure to specify your MCP server.
                Choose <span className="font-mono text-emerald-400">stdio</span> for process-based servers,{' '}
                <span className="font-mono text-emerald-400">Streamable</span> for MCP 2025+ HTTP servers, or{' '}
                <span className="font-mono text-emerald-400">SSE (Legacy)</span> for older HTTP+SSE servers.
              </p>
              <p className="text-xs text-zinc-500 mt-1.5">
                stdio example: <span className="font-mono text-emerald-400">python3 scripts/mock-mcp-server.py</span>
              </p>
              <p className="text-xs text-zinc-500 mt-0.5">
                Streamable HTTP example: <span className="font-mono text-emerald-400">http://localhost:8080</span> (MCP 2025-03-26 spec)
              </p>
            </div>
          </div>
        </div>
      )}
    </div>
  )
}
