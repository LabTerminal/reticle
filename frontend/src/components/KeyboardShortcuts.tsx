import { useEffect, useState } from 'react'
import { X, Keyboard } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { cn } from '@/lib/utils'

interface ShortcutItem {
  keys: string[]
  description: string
}

const shortcuts: ShortcutItem[] = [
  { keys: ['?'], description: 'Show this help' },
  { keys: ['Esc'], description: 'Close dialogs / Deselect message' },
  { keys: ['\u2191', '\u2193'], description: 'Navigate through messages' },
  { keys: ['\u2318', 'K'], description: 'Focus search (coming soon)' },
  { keys: ['\u2318', 'L'], description: 'Clear logs' },
  { keys: ['R'], description: 'Replay selected request' },
  { keys: ['E'], description: 'Edit and resend selected request' },
  { keys: ['C'], description: 'Copy selected message' },
]

const isMac = typeof navigator !== 'undefined' && navigator.platform.toUpperCase().indexOf('MAC') >= 0

export function KeyboardShortcuts() {
  const [isOpen, setIsOpen] = useState(false)

  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      // Show help on ? key (shift + /)
      if (e.key === '?' && !e.metaKey && !e.ctrlKey) {
        e.preventDefault()
        setIsOpen(true)
        return
      }

      // Close on Escape
      if (e.key === 'Escape' && isOpen) {
        e.preventDefault()
        setIsOpen(false)
        return
      }
    }

    window.addEventListener('keydown', handleKeyDown)
    return () => window.removeEventListener('keydown', handleKeyDown)
  }, [isOpen])

  if (!isOpen) return null

  return (
    <div className="fixed inset-0 z-[200] flex items-center justify-center">
      {/* Backdrop */}
      <div
        className="absolute inset-0 bg-black/60 backdrop-blur-sm"
        onClick={() => setIsOpen(false)}
      />

      {/* Modal */}
      <div className="relative bg-card border border-border rounded-xl shadow-2xl w-full max-w-md mx-4 overflow-hidden">
        {/* Header */}
        <div className="flex items-center justify-between px-5 py-4 border-b border-border bg-muted/30">
          <div className="flex items-center gap-2.5">
            <Keyboard className="w-5 h-5 text-[#00808F] dark:text-[#00F0FF]" />
            <h2 className="text-base font-semibold text-foreground">Keyboard Shortcuts</h2>
          </div>
          <Button
            variant="ghost"
            size="sm"
            onClick={() => setIsOpen(false)}
            className="h-8 w-8 p-0 hover:bg-muted"
          >
            <X className="w-4 h-4" />
          </Button>
        </div>

        {/* Shortcuts List */}
        <div className="p-4 space-y-2 max-h-[60vh] overflow-y-auto">
          {shortcuts.map((shortcut, index) => (
            <div
              key={index}
              className="flex items-center justify-between py-2 px-3 rounded-lg hover:bg-muted/50 transition-colors"
            >
              <span className="text-sm text-muted-foreground">{shortcut.description}</span>
              <div className="flex items-center gap-1">
                {shortcut.keys.map((key, keyIndex) => (
                  <span key={keyIndex}>
                    <kbd
                      className={cn(
                        'inline-flex items-center justify-center min-w-[24px] h-6 px-1.5',
                        'text-xs font-mono font-medium',
                        'bg-muted border border-border rounded',
                        'text-foreground shadow-sm'
                      )}
                    >
                      {key === '\u2318' ? (isMac ? '\u2318' : 'Ctrl') : key}
                    </kbd>
                    {keyIndex < shortcut.keys.length - 1 && (
                      <span className="mx-0.5 text-muted-foreground text-xs">+</span>
                    )}
                  </span>
                ))}
              </div>
            </div>
          ))}
        </div>

        {/* Footer */}
        <div className="px-5 py-3 border-t border-border bg-muted/20">
          <p className="text-xs text-muted-foreground text-center">
            Press <kbd className="px-1.5 py-0.5 bg-muted border border-border rounded text-[10px] font-mono">Esc</kbd> to close
          </p>
        </div>
      </div>
    </div>
  )
}
