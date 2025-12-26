import { useState } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { Loader2, Server, Wrench, FileText, Database, AlertCircle, Coins } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { ScrollArea } from '@/components/ui/scroll-area'
import type { ServerAnalysis } from '@/types'

export function ServerAnalyzer() {
  const [command, setCommand] = useState('')
  const [args, setArgs] = useState('')
  const [isAnalyzing, setIsAnalyzing] = useState(false)
  const [analysis, setAnalysis] = useState<ServerAnalysis | null>(null)
  const [error, setError] = useState<string | null>(null)

  const handleAnalyze = async () => {
    if (!command.trim()) return

    setIsAnalyzing(true)
    setError(null)
    setAnalysis(null)

    try {
      const argsList = args.trim() ? args.split(/\s+/) : []
      const result = await invoke<ServerAnalysis>('analyze_mcp_server', {
        command: command.trim(),
        args: argsList,
        env: null,
        timeoutSecs: 30,
      })
      setAnalysis(result)
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    } finally {
      setIsAnalyzing(false)
    }
  }

  return (
    <div className="flex flex-col h-full bg-background">
      {/* Header */}
      <div className="px-4 py-3 border-b border-border">
        <h2 className="text-sm font-semibold flex items-center gap-2">
          <Server className="w-4 h-4" />
          MCP Server Analyzer
        </h2>
        <p className="text-xs text-muted-foreground mt-1">
          Calculate the context token overhead of an MCP server
        </p>
      </div>

      {/* Input Form */}
      <div className="p-4 border-b border-border space-y-3">
        <div>
          <label className="text-xs text-muted-foreground mb-1 block">Command</label>
          <Input
            placeholder="npx -y @modelcontextprotocol/server-filesystem"
            value={command}
            onChange={(e) => setCommand(e.target.value)}
            className="font-mono text-sm"
          />
        </div>
        <div>
          <label className="text-xs text-muted-foreground mb-1 block">Arguments</label>
          <Input
            placeholder="/path/to/allowed/directory"
            value={args}
            onChange={(e) => setArgs(e.target.value)}
            className="font-mono text-sm"
          />
        </div>
        <Button
          onClick={handleAnalyze}
          disabled={!command.trim() || isAnalyzing}
          className="w-full"
        >
          {isAnalyzing ? (
            <>
              <Loader2 className="w-4 h-4 mr-2 animate-spin" />
              Analyzing...
            </>
          ) : (
            <>
              <Coins className="w-4 h-4 mr-2" />
              Analyze Context Cost
            </>
          )}
        </Button>
      </div>

      {/* Results */}
      <ScrollArea className="flex-1">
        <div className="p-4">
          {error && (
            <div className="p-3 rounded-md bg-destructive/10 border border-destructive/30 text-destructive text-sm flex items-start gap-2">
              <AlertCircle className="w-4 h-4 mt-0.5 flex-shrink-0" />
              <span>{error}</span>
            </div>
          )}

          {analysis && (
            <div className="space-y-4">
              {/* Server Info */}
              <div className="p-3 rounded-md bg-muted/50 border border-border">
                <div className="flex items-center justify-between mb-2">
                  <span className="font-medium text-sm">{analysis.server_name}</span>
                  <span className="text-xs text-muted-foreground">v{analysis.server_version}</span>
                </div>
                <div className="text-xs text-muted-foreground">
                  Protocol: {analysis.protocol_version}
                </div>
              </div>

              {/* Total Context Cost */}
              <div className="p-4 rounded-md bg-[#F59E0B]/10 border border-[#F59E0B]/30">
                <div className="flex items-center justify-between">
                  <span className="text-sm font-medium">Total Context Cost</span>
                  <span className="text-2xl font-bold text-[#F59E0B] font-mono">
                    {analysis.total_context_tokens.toLocaleString()}
                  </span>
                </div>
                <p className="text-xs text-muted-foreground mt-1">
                  tokens added to context when this server is connected
                </p>
              </div>

              {/* Breakdown */}
              <div className="grid grid-cols-3 gap-2">
                <div className="p-3 rounded-md bg-muted/50 border border-border text-center">
                  <Wrench className="w-4 h-4 mx-auto mb-1 text-blue-500" />
                  <div className="font-mono font-bold text-sm">
                    {analysis.tools.total_tokens.toLocaleString()}
                  </div>
                  <div className="text-[10px] text-muted-foreground">
                    {analysis.tools.count} tools
                  </div>
                </div>
                <div className="p-3 rounded-md bg-muted/50 border border-border text-center">
                  <FileText className="w-4 h-4 mx-auto mb-1 text-green-500" />
                  <div className="font-mono font-bold text-sm">
                    {analysis.prompts.total_tokens.toLocaleString()}
                  </div>
                  <div className="text-[10px] text-muted-foreground">
                    {analysis.prompts.count} prompts
                  </div>
                </div>
                <div className="p-3 rounded-md bg-muted/50 border border-border text-center">
                  <Database className="w-4 h-4 mx-auto mb-1 text-purple-500" />
                  <div className="font-mono font-bold text-sm">
                    {analysis.resources.total_tokens.toLocaleString()}
                  </div>
                  <div className="text-[10px] text-muted-foreground">
                    {analysis.resources.count} resources
                  </div>
                </div>
              </div>

              {/* Tools Detail */}
              {analysis.tools.tools.length > 0 && (
                <div>
                  <h3 className="text-xs font-semibold mb-2 text-muted-foreground uppercase tracking-wider flex items-center gap-1.5">
                    <Wrench className="w-3 h-3" />
                    Tools by Token Cost
                  </h3>
                  <div className="space-y-1">
                    {analysis.tools.tools.map((tool) => (
                      <div
                        key={tool.name}
                        className="flex items-center justify-between p-2 rounded bg-muted/30 text-xs"
                      >
                        <div className="flex-1 min-w-0">
                          <div className="font-mono font-medium truncate">{tool.name}</div>
                          <div className="text-muted-foreground truncate text-[10px]">
                            {tool.description}
                          </div>
                        </div>
                        <div className="flex items-center gap-2 ml-2 flex-shrink-0">
                          <div className="text-[10px] text-muted-foreground">
                            <span className="text-blue-500">{tool.schema_tokens}</span> schema
                          </div>
                          <span className="font-mono font-bold text-[#F59E0B]">
                            {tool.total_tokens}
                          </span>
                        </div>
                      </div>
                    ))}
                  </div>
                </div>
              )}

              {/* Prompts Detail */}
              {analysis.prompts.prompts.length > 0 && (
                <div>
                  <h3 className="text-xs font-semibold mb-2 text-muted-foreground uppercase tracking-wider flex items-center gap-1.5">
                    <FileText className="w-3 h-3" />
                    Prompts by Token Cost
                  </h3>
                  <div className="space-y-1">
                    {analysis.prompts.prompts.map((prompt) => (
                      <div
                        key={prompt.name}
                        className="flex items-center justify-between p-2 rounded bg-muted/30 text-xs"
                      >
                        <div className="flex-1 min-w-0">
                          <div className="font-mono font-medium truncate">{prompt.name}</div>
                          {prompt.description && (
                            <div className="text-muted-foreground truncate text-[10px]">
                              {prompt.description}
                            </div>
                          )}
                        </div>
                        <span className="font-mono font-bold text-[#F59E0B] ml-2">
                          {prompt.total_tokens}
                        </span>
                      </div>
                    ))}
                  </div>
                </div>
              )}

              {/* Resources Detail */}
              {analysis.resources.resources.length > 0 && (
                <div>
                  <h3 className="text-xs font-semibold mb-2 text-muted-foreground uppercase tracking-wider flex items-center gap-1.5">
                    <Database className="w-3 h-3" />
                    Resources by Token Cost
                  </h3>
                  <div className="space-y-1">
                    {analysis.resources.resources.map((resource) => (
                      <div
                        key={resource.uri}
                        className="flex items-center justify-between p-2 rounded bg-muted/30 text-xs"
                      >
                        <div className="flex-1 min-w-0">
                          <div className="font-mono font-medium truncate">{resource.name}</div>
                          <div className="text-muted-foreground truncate text-[10px]">
                            {resource.uri}
                          </div>
                        </div>
                        <span className="font-mono font-bold text-[#F59E0B] ml-2">
                          {resource.total_tokens}
                        </span>
                      </div>
                    ))}
                  </div>
                </div>
              )}
            </div>
          )}

          {!analysis && !error && !isAnalyzing && (
            <div className="text-center text-muted-foreground text-sm py-8">
              <Server className="w-8 h-8 mx-auto mb-2 opacity-50" />
              <p>Enter an MCP server command to analyze its context cost</p>
            </div>
          )}
        </div>
      </ScrollArea>
    </div>
  )
}
