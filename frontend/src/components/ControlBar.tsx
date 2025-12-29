import { useState, useEffect } from 'react'
import { Play, Pause, Settings, Loader2, RotateCcw, Circle, Database, Download, Trash2, FileJson, FileSpreadsheet, FileText } from 'lucide-react'
import { toast } from 'sonner'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
  AlertDialogTrigger,
} from '@/components/ui/alert-dialog'
import { useReticleStore } from '@/store'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { save } from '@tauri-apps/plugin-dialog'
import { ThemeToggle } from '@/components/ThemeToggle'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'

// Transport type definitions
type TransportType = 'stdio' | 'remote'

interface RecordedSession {
  id: string
  name: string
  started_at: number
  ended_at: number
  message_count: number
  transport_type: string
}

export function ControlBar() {
  // Proxy state
  const [isProxyRunning, setIsProxyRunning] = useState(false)
  const [isProxyLoading, setIsProxyLoading] = useState(false)
  const [showConfig, setShowConfig] = useState(false)
  const [transportType, setTransportType] = useState<TransportType>('stdio')
  const [command, setCommand] = useState('python3')
  const [args, setArgs] = useState('scripts/mock-mcp-server.py')
  const [serverUrl, setServerUrl] = useState('http://localhost:8080')
  const [proxyPort, setProxyPort] = useState(3001)
  const [sessionName, setSessionName] = useState('')

  // Recording state - use store for global access
  const { isRecording, setRecording, clearLogs } = useReticleStore()
  const [isRecordingLoading, setRecordingLoading] = useState(false)
  const [recordedSessions, setRecordedSessions] = useState<RecordedSession[]>([])
  const [showSessions, setShowSessions] = useState(false)
  const [sessionToDelete, setSessionToDelete] = useState<RecordedSession | null>(null)

  // Load recorded sessions on mount
  useEffect(() => {
    loadSessions()

    let unsubscribeStarted: (() => void) | null = null
    let unsubscribeStopped: (() => void) | null = null

    const setupListeners = async () => {
      unsubscribeStarted = await listen<{ session_id: string; session_name: string }>(
        'recording-started',
        (event) => {
          setRecording(true)
          toast.success('Recording started', {
            description: `Session: ${event.payload.session_name}`,
          })
          checkRecordingStatus()
        }
      )

      unsubscribeStopped = await listen<{ message_count: number; duration_ms: number }>(
        'recording-stopped',
        () => {
          setRecording(false)
          toast.success('Recording stopped')
          checkRecordingStatus()
          loadSessions()
        }
      )
    }

    setupListeners()

    // Cleanup listeners on unmount
    return () => {
      if (unsubscribeStarted) unsubscribeStarted()
      if (unsubscribeStopped) unsubscribeStopped()
    }
  }, [])

  const loadSessions = async () => {
    try {
      const sessions = await invoke<RecordedSession[]>('list_recorded_sessions')

      // Deduplicate sessions by ID (just in case)
      const uniqueSessions = sessions.reduce((acc, session) => {
        if (!acc.find(s => s.id === session.id)) {
          acc.push(session)
        }
        return acc
      }, [] as RecordedSession[])

      setRecordedSessions(uniqueSessions)
    } catch (error) {
      console.error('Failed to load sessions:', error)
    }
  }

  const checkRecordingStatus = async () => {
    try {
      const status = await invoke<{
        is_recording: boolean
        session_id?: string
        message_count: number
        duration_seconds: number
      }>('get_recording_status')
      setRecording(status.is_recording)
    } catch (error) {
      console.error('Failed to check recording status:', error)
    }
  }

  // Proxy controls
  const startProxy = async () => {
    setIsProxyLoading(true)
    try {
      if (transportType === 'stdio') {
        // Use stdio transport via start_proxy_v2
        const argsList = args.split(' ').filter((a) => a.trim())
        const transportConfig = { type: 'stdio', command, args: argsList }
        const invokeArgs: { transportConfig: typeof transportConfig; sessionName?: string } = { transportConfig }
        if (sessionName.trim()) {
          invokeArgs.sessionName = sessionName.trim()
        }
        await invoke('start_proxy_v2', invokeArgs)
      } else {
        // Use remote transport with auto-detection via start_remote_proxy
        const invokeArgs: {
          serverUrl: string
          proxyPort: number
          sessionName?: string
          useLegacySse?: boolean
        } = {
          serverUrl,
          proxyPort,
        }
        if (sessionName.trim()) {
          invokeArgs.sessionName = sessionName.trim()
        }
        await invoke('start_remote_proxy', invokeArgs)
      }

      setIsProxyRunning(true)
      toast.success('Proxy started successfully')
      setShowConfig(false)
      setSessionName('') // Reset for next session
    } catch (error) {
      toast.error('Failed to start proxy', {
        description: error instanceof Error ? error.message : String(error),
      })
    } finally {
      setIsProxyLoading(false)
    }
  }

  const stopProxy = async () => {
    setIsProxyLoading(true)
    try {
      await invoke('stop_proxy')
      setIsProxyRunning(false)
      toast.success('Proxy stopped')
    } catch (error) {
      toast.error('Failed to stop proxy', {
        description: error instanceof Error ? error.message : 'Unknown error',
      })
    } finally {
      setIsProxyLoading(false)
    }
  }

  // Recording controls
  const startRecording = async () => {
    setRecordingLoading(true)
    try {
      await invoke('start_recording', { sessionName: null })
      toast.success('Recording started')
      checkRecordingStatus()
    } catch (error) {
      toast.error('Failed to start recording', {
        description: error instanceof Error ? error.message : 'Unknown error',
      })
    } finally {
      setRecordingLoading(false)
    }
  }

  const stopRecording = async () => {
    setRecordingLoading(true)
    try {
      await invoke('stop_recording')
      toast.success('Recording stopped and saved')
      loadSessions()
      checkRecordingStatus()
    } catch (error) {
      console.error('Stop recording error:', error)
      const errorMessage = typeof error === 'string' ? error : (error instanceof Error ? error.message : 'Unknown error')
      toast.error('Failed to stop recording', {
        description: errorMessage,
      })
    } finally {
      setRecordingLoading(false)
    }
  }

  const exportSession = async (sessionId: string, name: string, format: 'json' | 'csv' | 'har') => {
    try {
      // Sanitize filename: replace spaces with hyphens and remove special chars
      const sanitizedName = name
        .replace(/\s+/g, '-')
        .replace(/[^a-zA-Z0-9-_]/g, '')
        .toLowerCase()

      const extension = format === 'har' ? 'har' : format
      const filterName = format === 'json' ? 'JSON' : format === 'csv' ? 'CSV' : 'HAR'

      // Open save dialog
      const filePath = await save({
        defaultPath: `${sanitizedName}.${extension}`,
        filters: [{
          name: filterName,
          extensions: [extension]
        }]
      })

      if (!filePath) {
        return // User cancelled
      }

      // Export to selected path using appropriate command
      const command = format === 'json'
        ? 'export_session'
        : format === 'csv'
          ? 'export_session_csv'
          : 'export_session_har'

      await invoke(command, { sessionId, exportPath: filePath })
      toast.success(`Session exported as ${filterName}`, { description: `Saved to: ${filePath}` })
    } catch (error) {
      toast.error('Failed to export session', {
        description: error instanceof Error ? error.message : 'Unknown error',
      })
    }
  }

  const deleteSession = async (sessionId: string) => {
    try {
      await invoke('delete_recorded_session', { sessionId })
      toast.success('Session deleted')
      loadSessions()
    } catch (error) {
      toast.error('Failed to delete session', {
        description: error instanceof Error ? error.message : 'Unknown error',
      })
    }
  }

  return (
    <div className="border-b border-border bg-card/50 backdrop-blur-sm relative z-50">
      <div className="flex items-center justify-between px-4 py-2 gap-4">
        {/* Left: Proxy Controls */}
        <div className="flex items-center gap-2">
          {/* Transport Toggle - Surfaced for quick access */}
          <div className="flex items-center bg-muted/50 rounded-md p-0.5">
            <Button
              onClick={() => setTransportType('stdio')}
              disabled={isProxyRunning}
              size="sm"
              variant={transportType === 'stdio' ? 'default' : 'ghost'}
              className="h-7 px-2 text-xs rounded-sm"
            >
              Stdio
            </Button>
            <Button
              onClick={() => setTransportType('remote')}
              disabled={isProxyRunning}
              size="sm"
              variant={transportType === 'remote' ? 'default' : 'ghost'}
              className="h-7 px-2 text-xs rounded-sm"
            >
              Remote
            </Button>
          </div>

          <div className="relative">
            <Button
              onClick={() => setShowConfig(!showConfig)}
              disabled={isProxyRunning}
              size="sm"
              variant="outline"
              className="h-8 gap-1.5"
            >
              <Settings className="h-3.5 w-3.5" />
              <span className="text-xs">Config</span>
            </Button>

            {showConfig && (
              <div className="absolute top-full left-0 mt-1 z-[100] w-80 bg-card border border-border rounded-lg shadow-lg p-3">
                <div className="space-y-3">
                  {transportType === 'stdio' ? (
                    <>
                      <Input
                        value={command}
                        onChange={(e) => setCommand(e.target.value)}
                        placeholder="Command (e.g., python3)"
                        className="h-8 text-xs"
                      />
                      <Input
                        value={args}
                        onChange={(e) => setArgs(e.target.value)}
                        placeholder="Arguments"
                        className="h-8 text-xs"
                      />
                    </>
                  ) : (
                    <>
                      <Input
                        value={serverUrl}
                        onChange={(e) => setServerUrl(e.target.value)}
                        placeholder="Server URL (http://, https://, ws://)"
                        className="h-8 text-xs"
                      />
                      <div className="text-xs text-muted-foreground">
                        Transport auto-detected from URL scheme
                      </div>
                      <Input
                        type="number"
                        value={proxyPort}
                        onChange={(e) => setProxyPort(parseInt(e.target.value))}
                        placeholder="Local proxy port"
                        className="h-8 text-xs"
                      />
                    </>
                  )}

                  <Input
                    value={sessionName}
                    onChange={(e) => setSessionName(e.target.value)}
                    placeholder="Session name (optional)"
                    className="h-8 text-xs"
                  />

                  <Button onClick={startProxy} size="sm" className="w-full h-7 text-xs">
                    <Play className="h-3 w-3 mr-1" />
                    Start Proxy
                  </Button>
                </div>
              </div>
            )}
          </div>

          {isProxyRunning && (
            <Button
              onClick={stopProxy}
              disabled={isProxyLoading}
              size="sm"
              variant="destructive"
              className="h-8 gap-1.5"
            >
              {isProxyLoading ? (
                <Loader2 className="h-3.5 w-3.5 animate-spin" />
              ) : (
                <Pause className="h-3.5 w-3.5" />
              )}
              <span className="text-xs">Stop</span>
            </Button>
          )}

          <AlertDialog>
            <AlertDialogTrigger asChild>
              <Button
                size="sm"
                variant="ghost"
                className="h-8 gap-1.5"
              >
                <RotateCcw className="h-3.5 w-3.5" />
                <span className="text-xs">Clear</span>
              </Button>
            </AlertDialogTrigger>
            <AlertDialogContent>
              <AlertDialogHeader>
                <AlertDialogTitle>Clear all logs?</AlertDialogTitle>
                <AlertDialogDescription>
                  This will remove all captured messages from the current view. This action cannot be undone.
                </AlertDialogDescription>
              </AlertDialogHeader>
              <AlertDialogFooter>
                <AlertDialogCancel>Cancel</AlertDialogCancel>
                <AlertDialogAction onClick={clearLogs}>Clear Logs</AlertDialogAction>
              </AlertDialogFooter>
            </AlertDialogContent>
          </AlertDialog>
        </div>

        {/* Center: Recording Controls */}
        <div className="flex items-center gap-2">
          {!isRecording ? (
            <Button
              onClick={startRecording}
              disabled={isRecordingLoading}
              size="sm"
              variant="outline"
              className="h-8 gap-1.5"
            >
              {isRecordingLoading ? (
                <Loader2 className="h-3.5 w-3.5 animate-spin" />
              ) : (
                <Circle className="h-3.5 w-3.5 fill-[#FF003C] text-[#FF003C]" />
              )}
              <span className="text-xs">Record</span>
            </Button>
          ) : (
            <Button
              onClick={stopRecording}
              disabled={isRecordingLoading}
              size="sm"
              variant="destructive"
              className="h-8 gap-1.5 animate-pulse"
            >
              {isRecordingLoading ? (
                <Loader2 className="h-3.5 w-3.5 animate-spin" />
              ) : (
                <Circle className="h-3.5 w-3.5 fill-white" />
              )}
              <span className="text-xs">Stop Recording</span>
            </Button>
          )}

          <div className="relative">
            <Button
              onClick={() => setShowSessions(!showSessions)}
              size="sm"
              variant="ghost"
              className="h-8 gap-1.5"
            >
              <Database className="h-3.5 w-3.5" />
              <span className="text-xs">Sessions ({recordedSessions.length})</span>
            </Button>

            {showSessions && (
              <div className="absolute top-full right-0 mt-1 z-[100] w-96 max-h-80 overflow-y-auto bg-card border border-border rounded-lg shadow-lg">
                {recordedSessions.length === 0 ? (
                  <div className="p-4 text-center text-sm text-muted-foreground">
                    No recorded sessions
                  </div>
                ) : (
                  <div className="divide-y divide-border">
                    {recordedSessions.map((session) => (
                      <div key={session.id} className="p-3 hover:bg-muted/50">
                        <div className="flex items-start justify-between gap-2">
                          <div className="flex-1 min-w-0">
                            <div className="text-sm font-medium text-foreground truncate">
                              {session.name}
                            </div>
                            <div className="text-xs text-muted-foreground mt-0.5">
                              {session.message_count} messages • {session.transport_type}
                            </div>
                            <div className="text-xs text-muted-foreground/70">
                              {new Date(session.started_at).toLocaleString()}
                            </div>
                          </div>
                          <div className="flex gap-1">
                            <DropdownMenu>
                              <DropdownMenuTrigger asChild>
                                <Button
                                  size="sm"
                                  variant="ghost"
                                  className="h-7 w-7 p-0"
                                >
                                  <Download className="h-3.5 w-3.5" />
                                </Button>
                              </DropdownMenuTrigger>
                              <DropdownMenuContent align="end">
                                <DropdownMenuItem onClick={() => exportSession(session.id, session.name, 'json')}>
                                  <FileJson className="h-4 w-4 mr-2" />
                                  Export as JSON
                                </DropdownMenuItem>
                                <DropdownMenuItem onClick={() => exportSession(session.id, session.name, 'csv')}>
                                  <FileSpreadsheet className="h-4 w-4 mr-2" />
                                  Export as CSV
                                </DropdownMenuItem>
                                <DropdownMenuItem onClick={() => exportSession(session.id, session.name, 'har')}>
                                  <FileText className="h-4 w-4 mr-2" />
                                  Export as HAR
                                </DropdownMenuItem>
                              </DropdownMenuContent>
                            </DropdownMenu>
                            <Button
                              onClick={() => setSessionToDelete(session)}
                              size="sm"
                              variant="ghost"
                              className="h-7 w-7 p-0 text-[#FF003C] hover:text-[#FF003C]/80"
                            >
                              <Trash2 className="h-3.5 w-3.5" />
                            </Button>
                          </div>
                        </div>
                      </div>
                    ))}
                  </div>
                )}
              </div>
            )}
          </div>
        </div>

        {/* Right: Theme Toggle */}
        <ThemeToggle />
      </div>

      {/* Delete Session Confirmation Dialog */}
      <AlertDialog open={!!sessionToDelete} onOpenChange={(open) => !open && setSessionToDelete(null)}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Delete session?</AlertDialogTitle>
            <AlertDialogDescription>
              This will permanently delete "{sessionToDelete?.name}" and all its recorded messages. This action cannot be undone.
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>Cancel</AlertDialogCancel>
            <AlertDialogAction
              onClick={() => {
                if (sessionToDelete) {
                  deleteSession(sessionToDelete.id)
                  setSessionToDelete(null)
                }
              }}
              className="bg-[#FF003C] hover:bg-[#FF003C]/90"
            >
              Delete
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  )
}
