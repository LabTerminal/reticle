import { useState, useEffect, useCallback } from 'react'
import { invoke } from '@tauri-apps/api/core'
import Editor from '@monaco-editor/react'
import { Send, Zap, ChevronDown, ChevronUp, Loader2, AlertCircle } from 'lucide-react'
import { toast } from 'sonner'
import { Button } from '@/components/ui/button'
import { cn } from '@/lib/utils'
import { useTheme } from '@/components/theme-provider'

interface McpMethodInfo {
  method: string
  description: string
  example_params: Record<string, unknown> | null
}

interface SendRequestParams {
  method: string
  params?: unknown
  id?: unknown
}

interface SendRequestResult {
  request_id: unknown
  request: unknown
}

export function RequestComposer() {
  const [isExpanded, setIsExpanded] = useState(false)
  const [canInteract, setCanInteract] = useState(false)
  const [methods, setMethods] = useState<McpMethodInfo[]>([])
  const [selectedMethod, setSelectedMethod] = useState<string>('')
  const [paramsJson, setParamsJson] = useState<string>('')
  const [isSending, setIsSending] = useState(false)
  const [showMethodDropdown, setShowMethodDropdown] = useState(false)
  const { resolvedTheme } = useTheme()

  // Check if interaction is available
  const checkInteractionStatus = useCallback(async () => {
    try {
      const result = await invoke<boolean>('can_interact')
      setCanInteract(result)
    } catch {
      setCanInteract(false)
    }
  }, [])

  // Load MCP methods
  const loadMethods = useCallback(async () => {
    try {
      const result = await invoke<McpMethodInfo[]>('get_mcp_methods')
      setMethods(result)
    } catch (error) {
      console.error('Failed to load MCP methods:', error)
    }
  }, [])

  useEffect(() => {
    loadMethods()
    checkInteractionStatus()

    // Poll for interaction status
    const interval = setInterval(checkInteractionStatus, 2000)
    return () => clearInterval(interval)
  }, [loadMethods, checkInteractionStatus])

  // Update params when method changes
  const handleMethodSelect = (method: McpMethodInfo) => {
    setSelectedMethod(method.method)
    setShowMethodDropdown(false)

    if (method.example_params) {
      setParamsJson(JSON.stringify(method.example_params, null, 2))
    } else {
      setParamsJson('')
    }
  }

  // Send request
  const handleSend = async () => {
    if (!selectedMethod.trim()) {
      toast.error('Please select or enter a method')
      return
    }

    setIsSending(true)

    try {
      // Parse params if provided
      let params: unknown = undefined
      if (paramsJson.trim()) {
        try {
          params = JSON.parse(paramsJson)
        } catch (e) {
          toast.error('Invalid JSON in parameters')
          setIsSending(false)
          return
        }
      }

      const requestParams: SendRequestParams = {
        method: selectedMethod,
        params,
      }

      const result = await invoke<SendRequestResult>('send_request', { params: requestParams })

      toast.success('Request sent', {
        description: `Method: ${selectedMethod}`,
      })

      console.log('Request sent:', result)
    } catch (error) {
      const errorMessage = typeof error === 'string' ? error : (error instanceof Error ? error.message : 'Unknown error')
      toast.error('Failed to send request', {
        description: errorMessage,
      })
    } finally {
      setIsSending(false)
    }
  }

  // Send raw JSON
  const handleSendRaw = async () => {
    const rawJson = paramsJson.trim()
    if (!rawJson) {
      toast.error('Please enter a JSON-RPC message')
      return
    }

    setIsSending(true)

    try {
      // Validate JSON
      JSON.parse(rawJson)

      await invoke('send_raw_message', { message: rawJson })

      toast.success('Raw message sent')
    } catch (error) {
      const errorMessage = typeof error === 'string' ? error : (error instanceof Error ? error.message : 'Unknown error')
      toast.error('Failed to send message', {
        description: errorMessage,
      })
    } finally {
      setIsSending(false)
    }
  }

  return (
    <div className="border-t border-border bg-card/60">
      {/* Header - Always visible */}
      <button
        onClick={() => setIsExpanded(!isExpanded)}
        className="w-full flex items-center justify-between px-4 py-2 hover:bg-muted/40 transition-colors"
      >
        <div className="flex items-center gap-2">
          <Zap className={cn(
            "w-4 h-4",
            canInteract ? "text-[#059669] dark:text-[#00FF9F]" : "text-muted-foreground"
          )} />
          <span className="text-sm font-medium text-foreground">Interact</span>
          {!canInteract && (
            <span className="text-xs text-muted-foreground ml-2">(Start a proxy to enable)</span>
          )}
        </div>
        {isExpanded ? (
          <ChevronUp className="w-4 h-4 text-muted-foreground" />
        ) : (
          <ChevronDown className="w-4 h-4 text-muted-foreground" />
        )}
      </button>

      {/* Expanded Content */}
      {isExpanded && (
        <div className="px-4 pb-4 space-y-3">
          {!canInteract ? (
            <div className="flex items-center gap-2 text-xs text-[#D97706] dark:text-[#FCEE09] bg-[#D97706]/10 dark:bg-[#FCEE09]/10 border border-[#D97706]/30 dark:border-[#FCEE09]/30 px-3 py-2 rounded-md">
              <AlertCircle className="w-4 h-4" />
              <span>Start a proxy (stdio or HTTP/SSE) to send requests to the MCP server</span>
            </div>
          ) : (
            <>
              {/* Method Selector */}
              <div className="relative">
                <label className="text-xs text-muted-foreground mb-1 block">Method</label>
                <button
                  onClick={() => setShowMethodDropdown(!showMethodDropdown)}
                  className="w-full flex items-center justify-between px-3 py-2 bg-muted border border-border rounded-md text-sm text-foreground hover:bg-muted/80 transition-colors"
                >
                  <span className={selectedMethod ? 'text-foreground' : 'text-muted-foreground'}>
                    {selectedMethod || 'Select a method...'}
                  </span>
                  <ChevronDown className="w-4 h-4 text-muted-foreground" />
                </button>

                {showMethodDropdown && (
                  <div className="absolute z-50 w-full mt-1 bg-popover border border-border rounded-md shadow-xl max-h-60 overflow-y-auto">
                    {methods.map((method) => (
                      <button
                        key={method.method}
                        onClick={() => handleMethodSelect(method)}
                        className="w-full px-3 py-2 text-left hover:bg-muted transition-colors"
                      >
                        <div className="text-sm text-foreground font-mono">{method.method}</div>
                        <div className="text-xs text-muted-foreground mt-0.5">{method.description}</div>
                      </button>
                    ))}
                  </div>
                )}
              </div>

              {/* Custom Method Input */}
              <div>
                <label className="text-xs text-muted-foreground mb-1 block">Or enter custom method</label>
                <input
                  type="text"
                  value={selectedMethod}
                  onChange={(e) => setSelectedMethod(e.target.value)}
                  placeholder="e.g., tools/list"
                  className="w-full px-3 py-2 bg-muted border border-border rounded-md text-sm text-foreground placeholder:text-muted-foreground focus:outline-none focus:ring-1 focus:ring-ring"
                />
              </div>

              {/* Parameters Editor */}
              <div>
                <label className="text-xs text-muted-foreground mb-1 block">Parameters (JSON)</label>
                <div className="h-32 border border-border rounded-md overflow-hidden">
                  <Editor
                    height="100%"
                    defaultLanguage="json"
                    value={paramsJson}
                    onChange={(value) => setParamsJson(value || '')}
                    theme={resolvedTheme === 'dark' ? 'vs-dark' : 'light'}
                    options={{
                      minimap: { enabled: false },
                      fontSize: 12,
                      fontFamily: 'JetBrains Mono, Geist Mono, monospace',
                      lineNumbers: 'off',
                      scrollBeyondLastLine: false,
                      automaticLayout: true,
                      wordWrap: 'on',
                      folding: false,
                      scrollbar: {
                        vertical: 'auto',
                        horizontal: 'auto',
                        verticalScrollbarSize: 6,
                        horizontalScrollbarSize: 6,
                      },
                      padding: { top: 8, bottom: 8 },
                    }}
                  />
                </div>
              </div>

              {/* Action Buttons */}
              <div className="flex items-center gap-2">
                <Button
                  onClick={handleSend}
                  disabled={isSending || !selectedMethod.trim()}
                  className="flex-1 bg-primary hover:bg-primary/90 text-primary-foreground"
                >
                  {isSending ? (
                    <Loader2 className="w-4 h-4 mr-2 animate-spin" />
                  ) : (
                    <Send className="w-4 h-4 mr-2" />
                  )}
                  Send Request
                </Button>
                <Button
                  onClick={handleSendRaw}
                  disabled={isSending || !paramsJson.trim()}
                  variant="outline"
                  className="border-border hover:bg-muted"
                >
                  Send Raw
                </Button>
              </div>

              {/* Quick Actions */}
              <div className="pt-2 border-t border-border">
                <div className="text-xs text-muted-foreground mb-2">Quick Actions</div>
                <div className="flex flex-wrap gap-2">
                  {['initialize', 'tools/list', 'resources/list', 'prompts/list', 'ping'].map((method) => (
                    <button
                      key={method}
                      onClick={() => {
                        const methodInfo = methods.find((m) => m.method === method)
                        if (methodInfo) {
                          handleMethodSelect(methodInfo)
                        } else {
                          setSelectedMethod(method)
                          setParamsJson('')
                        }
                      }}
                      className="px-2 py-1 text-xs font-mono bg-muted hover:bg-muted/80 border border-border rounded-md text-foreground transition-colors"
                    >
                      {method}
                    </button>
                  ))}
                </div>
              </div>
            </>
          )}
        </div>
      )}
    </div>
  )
}
