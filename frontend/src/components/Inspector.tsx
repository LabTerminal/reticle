import { useEffect, useState } from 'react'
import { invoke } from '@tauri-apps/api/core'
import Editor from '@monaco-editor/react'
import { Copy, Check, FileJson, ArrowRight, AlertTriangle, Terminal, Play, Pencil, X, Send } from 'lucide-react'
import { toast } from 'sonner'
import {
  useReticleStore,
  parseLogMessage,
  findCorrelatedRequest,
  calculateLatency,
} from '@/store'
import { Button } from '@/components/ui/button'
import { cn, formatTimestamp, formatDuration } from '@/lib/utils'
import { useTheme } from '@/components/theme-provider'

export function Inspector() {
  const selectedLog = useReticleStore((state) => state.getSelectedLog())
  const logs = useReticleStore((state) => state.logs)
  const { selectLog } = useReticleStore()
  const [copied, setCopied] = useState(false)
  const [isEditMode, setIsEditMode] = useState(false)
  const [editedContent, setEditedContent] = useState('')
  const [isSending, setIsSending] = useState(false)
  const { resolvedTheme } = useTheme()

  // Check message type
  const isRawMessage = selectedLog?.message_type === 'raw'
  const isStderrMessage = selectedLog?.message_type === 'stderr'
  const isNonJsonRpc = isRawMessage || isStderrMessage

  // Check if this is a response and find correlated request
  const parsed = selectedLog && !isNonJsonRpc ? parseLogMessage(selectedLog) : null
  const isResponse =
    parsed && !parsed.method && (parsed.result !== undefined || parsed.error !== undefined)
  const correlatedRequest = isResponse && selectedLog ? findCorrelatedRequest(selectedLog, logs) : null
  const latency =
    correlatedRequest && selectedLog ? calculateLatency(correlatedRequest, selectedLog) : null

  // Reset state when selection changes
  useEffect(() => {
    setCopied(false)
    setIsEditMode(false)
    setEditedContent('')
  }, [selectedLog?.id])

  // Check if this is a replayable request (incoming JSON-RPC with method)
  const isReplayable = selectedLog &&
    !isNonJsonRpc &&
    selectedLog.direction === 'in' &&
    parsed?.method

  // Replay the exact message
  const handleReplay = async () => {
    if (!selectedLog) return

    setIsSending(true)
    try {
      await invoke('send_raw_message', { message: selectedLog.content })
      toast.success('Request replayed', {
        description: `Method: ${parsed?.method}`,
      })
    } catch (error) {
      const errorMessage = typeof error === 'string' ? error : (error instanceof Error ? error.message : 'Unknown error')
      toast.error('Failed to replay request', {
        description: errorMessage,
      })
    } finally {
      setIsSending(false)
    }
  }

  // Enter edit mode
  const handleEditMode = () => {
    if (!selectedLog) return
    setEditedContent(formatJSON(selectedLog.content))
    setIsEditMode(true)
  }

  // Cancel edit mode
  const handleCancelEdit = () => {
    setIsEditMode(false)
    setEditedContent('')
  }

  // Send edited message
  const handleSendEdited = async () => {
    if (!editedContent.trim()) return

    // Validate JSON
    try {
      JSON.parse(editedContent)
    } catch {
      toast.error('Invalid JSON')
      return
    }

    setIsSending(true)
    try {
      await invoke('send_raw_message', { message: editedContent })
      toast.success('Modified request sent')
      setIsEditMode(false)
      setEditedContent('')
    } catch (error) {
      const errorMessage = typeof error === 'string' ? error : (error instanceof Error ? error.message : 'Unknown error')
      toast.error('Failed to send request', {
        description: errorMessage,
      })
    } finally {
      setIsSending(false)
    }
  }

  const handleCopy = async () => {
    if (!selectedLog) return

    try {
      await navigator.clipboard.writeText(selectedLog.content)
      setCopied(true)
      toast.success('JSON copied to clipboard', {
        duration: 2000,
      })
      setTimeout(() => setCopied(false), 2000)
    } catch (err) {
      console.error('Failed to copy:', err)
      toast.error('Failed to copy to clipboard', {
        duration: 3000,
      })
    }
  }

  // Format JSON for display
  const formattedJSON = selectedLog
    ? formatJSON(selectedLog.content)
    : ''

  return (
    <div className="flex flex-col h-full bg-background border-l border-border">
      {/* Header - Premium Glass Panel */}
      <div className="glass-strong flex items-center justify-between px-4 py-2.5 border-b border-border">
        <div className="flex items-center gap-2">
          {isStderrMessage ? (
            <AlertTriangle className="w-4 h-4 text-[#DC2626] dark:text-[#FF003C]" />
          ) : isRawMessage ? (
            <Terminal className="w-4 h-4 text-[#D97706] dark:text-[#FCEE09]" />
          ) : (
            <FileJson className="w-4 h-4 text-[#00808F] dark:text-[#00F0FF]" />
          )}
          <h2 className="text-sm font-semibold text-foreground">
            {isStderrMessage ? 'Stderr Output' : isRawMessage ? 'Raw Output' : 'Inspector'}
          </h2>
        </div>
        {selectedLog && (
          <div className="flex items-center gap-2">
            {/* Replay/Edit buttons for replayable requests */}
            {isReplayable && !isEditMode && (
              <>
                <Button
                  variant="ghost"
                  size="sm"
                  onClick={handleReplay}
                  disabled={isSending}
                  className="h-9 px-3 border border-[#059669]/50 dark:border-[#00FF9F]/50 hover:bg-[#059669]/10 dark:hover:bg-[#00FF9F]/10"
                  title="Replay this request"
                >
                  <Play className="w-3.5 h-3.5 mr-2 text-[#059669] dark:text-[#00FF9F]" />
                  <span className="text-xs text-[#059669] dark:text-[#00FF9F]">Replay</span>
                </Button>
                <Button
                  variant="ghost"
                  size="sm"
                  onClick={handleEditMode}
                  className="h-9 px-3 border border-[#00808F]/50 dark:border-[#00F0FF]/50 hover:bg-[#00808F]/10 dark:hover:bg-[#00F0FF]/10"
                  title="Edit and resend"
                >
                  <Pencil className="w-3.5 h-3.5 mr-2 text-[#00808F] dark:text-[#00F0FF]" />
                  <span className="text-xs text-[#00808F] dark:text-[#00F0FF]">Edit</span>
                </Button>
              </>
            )}
            {/* Edit mode actions */}
            {isEditMode && (
              <>
                <Button
                  variant="ghost"
                  size="sm"
                  onClick={handleSendEdited}
                  disabled={isSending}
                  className="h-9 px-3 border border-[#059669]/50 dark:border-[#00FF9F]/50 hover:bg-[#059669]/10 dark:hover:bg-[#00FF9F]/10"
                >
                  <Send className="w-3.5 h-3.5 mr-2 text-[#059669] dark:text-[#00FF9F]" />
                  <span className="text-xs text-[#059669] dark:text-[#00FF9F]">Send</span>
                </Button>
                <Button
                  variant="ghost"
                  size="sm"
                  onClick={handleCancelEdit}
                  className="h-9 px-3 border border-border hover:bg-muted"
                >
                  <X className="w-3.5 h-3.5 mr-2 text-muted-foreground" />
                  <span className="text-xs text-foreground">Cancel</span>
                </Button>
              </>
            )}
            {/* Copy button */}
            <Button
              variant="ghost"
              size="sm"
              onClick={handleCopy}
              className="h-9 px-3 border border-border hover:bg-muted"
            >
              {copied ? (
                <>
                  <Check className="w-3.5 h-3.5 mr-2 text-[#059669] dark:text-[#00FF9F]" />
                  <span className="text-xs text-[#059669] dark:text-[#00FF9F]">Copied</span>
                </>
              ) : (
                <>
                  <Copy className="w-3.5 h-3.5 mr-2 text-muted-foreground" />
                  <span className="text-xs text-foreground">Copy</span>
                </>
              )}
            </Button>
          </div>
        )}
      </div>

      {/* Content */}
      {selectedLog ? (
        <div className="flex flex-col flex-1 overflow-hidden">
          {/* Metadata - Premium Badge Layout */}
          <div className="px-4 py-3 bg-muted/40 border-b border-border space-y-2">
            {/* Warning banner for stderr */}
            {isStderrMessage && (
              <div className="flex items-center gap-2 px-3 py-2 bg-[#DC2626]/10 dark:bg-[#FF003C]/10 border border-[#DC2626]/30 dark:border-[#FF003C]/30 rounded-md mb-2">
                <AlertTriangle className="w-4 h-4 text-[#DC2626] dark:text-[#FF003C] flex-shrink-0" />
                <span className="text-xs text-[#DC2626] dark:text-[#FF003C]">
                  This is stderr output from the MCP server (errors, warnings, tracebacks)
                </span>
              </div>
            )}
            {isRawMessage && (
              <div className="flex items-center gap-2 px-3 py-2 bg-[#D97706]/10 dark:bg-[#FCEE09]/10 border border-[#D97706]/30 dark:border-[#FCEE09]/30 rounded-md mb-2">
                <Terminal className="w-4 h-4 text-[#D97706] dark:text-[#FCEE09] flex-shrink-0" />
                <span className="text-xs text-[#D97706] dark:text-[#FCEE09]">
                  This is raw stdout output (non-JSON-RPC data)
                </span>
              </div>
            )}
            <div className="flex items-center justify-between text-xs">
              <span className="text-muted-foreground font-medium">Timestamp</span>
              <span className="font-mono text-foreground tabular-nums">
                {formatTimestamp(selectedLog.timestamp)}
              </span>
            </div>
            <div className="flex items-center justify-between text-xs">
              <span className="text-muted-foreground font-medium">Type</span>
              <span
                className={cn(
                  'inline-flex items-center px-1.5 py-0.5 rounded-md font-mono font-medium border',
                  isStderrMessage
                    ? 'bg-[#DC2626]/20 dark:bg-[#FF003C]/20 text-[#DC2626] dark:text-[#FF003C] border-[#DC2626]/30 dark:border-[#FF003C]/30'
                    : isRawMessage
                    ? 'bg-[#D97706]/20 dark:bg-[#FCEE09]/20 text-[#D97706] dark:text-[#FCEE09] border-[#D97706]/30 dark:border-[#FCEE09]/30'
                    : 'bg-secondary text-secondary-foreground border-border'
                )}
              >
                {isStderrMessage ? 'stderr' : isRawMessage ? 'raw' : 'json-rpc'}
              </span>
            </div>
            <div className="flex items-center justify-between text-xs">
              <span className="text-muted-foreground font-medium">Direction</span>
              <span
                className={cn(
                  'inline-flex items-center px-1.5 py-0.5 rounded-md font-mono font-medium bg-secondary border border-border',
                  selectedLog.direction === 'in'
                    ? 'text-[#00808F] dark:text-[#00F0FF]'
                    : 'text-[#059669] dark:text-[#00FF9F]'
                )}
              >
                {selectedLog.direction === 'in' ? 'Incoming' : 'Outgoing'}
              </span>
            </div>
            <div className="flex items-center justify-between text-xs">
              <span className="text-muted-foreground font-medium">Session</span>
              <span
                className="font-mono text-muted-foreground text-[11px] truncate max-w-[180px]"
                title={selectedLog.session_id}
              >
                ...{selectedLog.session_id.slice(-8)}
              </span>
            </div>
            {(latency !== null || selectedLog.duration_micros !== undefined) && (
              <div className="flex items-center justify-between text-xs">
                <span className="text-muted-foreground font-medium">
                  {latency !== null ? 'Round-trip Latency' : 'Duration'}
                </span>
                <span
                  className={cn(
                    'font-mono font-semibold tabular-nums',
                    (latency || selectedLog.duration_micros || 0) > 1000000
                      ? 'text-[#DC2626] dark:text-[#FF003C]'
                      : (latency || selectedLog.duration_micros || 0) > 100000
                      ? 'text-[#D97706] dark:text-[#FCEE09]'
                      : 'text-[#059669] dark:text-[#00FF9F]'
                  )}
                >
                  {formatDuration(latency || selectedLog.duration_micros || 0)}
                </span>
              </div>
            )}

            {/* Correlated Request Info */}
            {correlatedRequest && (
              <div className="pt-2 mt-2 border-t border-border">
                <div className="flex items-center justify-between text-xs mb-2">
                  <span className="text-muted-foreground font-medium">Correlated Request</span>
                  <Button
                    variant="ghost"
                    size="sm"
                    onClick={() => selectLog(correlatedRequest.id)}
                    className="h-8 px-3 text-xs border border-border hover:bg-muted"
                  >
                    <ArrowRight className="w-3.5 h-3.5 mr-1.5 text-[#00808F] dark:text-[#00F0FF]" />
                    <span className="text-foreground">Jump</span>
                  </Button>
                </div>
                <div className="flex items-center justify-between text-xs">
                  <span className="text-muted-foreground">Request ID</span>
                  <span className="font-mono text-foreground">#{parsed?.id}</span>
                </div>
                <div className="flex items-center justify-between text-xs mt-1">
                  <span className="text-muted-foreground">Method</span>
                  <span className="inline-flex items-center px-1.5 py-0.5 rounded-md font-mono text-[11px] font-medium bg-secondary text-[#00808F] dark:text-[#00F0FF] border border-border">
                    {parseLogMessage(correlatedRequest)?.method}
                  </span>
                </div>
              </div>
            )}
          </div>

          {/* Monaco Editor - Premium Container */}
          <div className="flex-1 overflow-hidden">
            {isEditMode && (
              <div className="px-4 py-2 bg-[#00808F]/10 dark:bg-[#00F0FF]/10 border-b border-[#00808F]/30 dark:border-[#00F0FF]/30">
                <span className="text-xs text-[#00808F] dark:text-[#00F0FF] font-medium">
                  Edit Mode — Modify the JSON and click Send to replay with changes
                </span>
              </div>
            )}
            <Editor
              height="100%"
              defaultLanguage={isNonJsonRpc ? 'plaintext' : 'json'}
              value={isEditMode ? editedContent : (isNonJsonRpc ? selectedLog.content : formattedJSON)}
              onChange={(value) => isEditMode && setEditedContent(value || '')}
              theme={resolvedTheme === 'dark' ? 'vs-dark' : 'light'}
              options={{
                readOnly: !isEditMode,
                minimap: { enabled: false },
                fontSize: 12,
                fontFamily: 'JetBrains Mono, Geist Mono, monospace',
                lineNumbers: 'on',
                scrollBeyondLastLine: false,
                automaticLayout: true,
                wordWrap: 'on',
                folding: !isNonJsonRpc,
                renderLineHighlight: 'all',
                scrollbar: {
                  vertical: 'auto',
                  horizontal: 'auto',
                  verticalScrollbarSize: 6,
                  horizontalScrollbarSize: 6,
                },
                padding: {
                  top: 16,
                  bottom: 16,
                },
              }}
            />
          </div>
        </div>
      ) : (
        <div className="flex items-center justify-center h-full">
          <div className="text-center max-w-xs">
            <div className="w-12 h-12 mx-auto mb-3 rounded-xl bg-muted/50 flex items-center justify-center">
              <FileJson className="w-6 h-6 text-muted-foreground/50" />
            </div>
            <p className="text-sm text-foreground font-medium mb-1">Select a message</p>
            <p className="text-xs text-muted-foreground">
              Click any message in the stream to view its full JSON content, metadata, and latency info.
            </p>
            <p className="text-[11px] text-muted-foreground/60 mt-3">
              Use <kbd className="px-1 py-0.5 bg-muted border border-border rounded text-[10px] font-mono">↑</kbd> <kbd className="px-1 py-0.5 bg-muted border border-border rounded text-[10px] font-mono">↓</kbd> to navigate
            </p>
          </div>
        </div>
      )}
    </div>
  )
}

/**
 * Format JSON string with proper indentation
 */
function formatJSON(jsonStr: string): string {
  try {
    const parsed = JSON.parse(jsonStr)
    return JSON.stringify(parsed, null, 2)
  } catch {
    return jsonStr
  }
}
