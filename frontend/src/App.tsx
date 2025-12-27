import { useEffect } from 'react'
import { listen } from '@tauri-apps/api/event'
import {
  Panel,
  PanelGroup,
  PanelResizeHandle,
} from 'react-resizable-panels'
import { Toaster, toast } from 'sonner'
import { LogStream } from '@/components/LogStream'
import { Inspector } from '@/components/Inspector'
import { Sidebar } from '@/components/Sidebar'
import { ControlBar } from '@/components/ControlBar'
import { RequestComposer } from '@/components/RequestComposer'
import { KeyboardShortcuts } from '@/components/KeyboardShortcuts'
import { useReticleStore } from '@/store'
import { useTheme } from '@/components/theme-provider'
import { LogEntry } from '@/types'
import './styles/globals.css'

function App() {
  const { addLog, setConnected, addSession } = useReticleStore()
  const { resolvedTheme } = useTheme()

  useEffect(() => {
    // Set initial connection state
    setConnected(true)

    let unsubscribe: (() => void) | null = null
    let unsubscribeSession: (() => void) | null = null

    // Set up listeners
    const setupListeners = async () => {
      console.log('Setting up Tauri event listeners...')

      try {
        // Listen for log events from Tauri backend
        unsubscribe = await listen<LogEntry>('log-event', (event) => {
          console.log('Received log-event:', event.payload.id)
          const log = event.payload

          // Extract method from JSON if not provided
          if (!log.method) {
            try {
              const parsed = JSON.parse(log.content)
              log.method = parsed.method
            } catch {
              // Ignore parse errors
            }
          }

          addLog(log)
        })

        // Listen for session events
        unsubscribeSession = await listen<{
          id: string
          started_at: number
        }>('session-start', (event) => {
          console.log('Received session-start:', event.payload.id)
          addSession({
            id: event.payload.id,
            started_at: event.payload.started_at,
            message_count: 0,
            last_activity: event.payload.started_at,
          })
        })

        console.log('Tauri event listeners ready!')
      } catch (error) {
        console.error('Failed to set up event listeners:', error)
        toast.error('Connection error', {
          description: 'Failed to connect to backend. Please restart the application.',
          duration: 5000,
        })
        setConnected(false)
      }
    }

    setupListeners()

    // Cleanup listeners on unmount
    return () => {
      if (unsubscribe) unsubscribe()
      if (unsubscribeSession) unsubscribeSession()
    }
  }, [addLog, setConnected, addSession])

  return (
    <div className="h-screen w-screen bg-background text-foreground overflow-hidden flex flex-col">
      {/* Toast Notifications */}
      <Toaster
        theme={resolvedTheme}
        position="top-right"
        toastOptions={{
          className: 'font-sans text-sm',
        }}
      />

      {/* Keyboard Shortcuts Help Modal */}
      <KeyboardShortcuts />

      {/* Unified Control Bar */}
      <ControlBar />

      {/* Main Layout */}
      <PanelGroup direction="horizontal" className="flex-1">
        {/* Left Sidebar - Metrics & Filters */}
        <Panel
          defaultSize={20}
          minSize={15}
          maxSize={30}
          className="min-w-[250px]"
        >
          <Sidebar />
        </Panel>

        <PanelResizeHandle className="w-2 bg-border/50 hover:bg-[#00808F]/30 dark:hover:bg-[#00F0FF]/30 transition-all duration-200 data-[resize-handle-active]:bg-[#00808F] dark:data-[resize-handle-active]:bg-[#00F0FF] relative group">
          <div className="absolute inset-y-0 left-1/2 -translate-x-1/2 w-1 bg-border group-hover:bg-[#00808F]/50 dark:group-hover:bg-[#00F0FF]/50 transition-colors" />
        </PanelResizeHandle>

        {/* Center - Log Stream */}
        <Panel defaultSize={50} minSize={30}>
          <LogStream />
        </Panel>

        <PanelResizeHandle className="w-2 bg-border/50 hover:bg-[#00808F]/30 dark:hover:bg-[#00F0FF]/30 transition-all duration-200 data-[resize-handle-active]:bg-[#00808F] dark:data-[resize-handle-active]:bg-[#00F0FF] relative group">
          <div className="absolute inset-y-0 left-1/2 -translate-x-1/2 w-1 bg-border group-hover:bg-[#00808F]/50 dark:group-hover:bg-[#00F0FF]/50 transition-colors" />
        </PanelResizeHandle>

        {/* Right - Inspector + Request Composer */}
        <Panel
          defaultSize={30}
          minSize={25}
          maxSize={50}
          className="min-w-[350px]"
        >
          <div className="flex flex-col h-full">
            <div className="flex-1 overflow-hidden">
              <Inspector />
            </div>
            <RequestComposer />
          </div>
        </Panel>
      </PanelGroup>
    </div>
  )
}

export default App
