import { useState } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { X, Plus, Tag, Server } from 'lucide-react'
import { toast } from 'sonner'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { cn } from '@/lib/utils'
import { useReticleStore } from '@/store'
import type { SessionInfo } from '@/types'

interface SessionTagsProps {
  session: SessionInfo
  onTagsUpdated?: () => void
}

export function SessionTags({ session, onTagsUpdated }: SessionTagsProps) {
  const [isAdding, setIsAdding] = useState(false)
  const [newTag, setNewTag] = useState('')
  const [isLoading, setIsLoading] = useState(false)
  const { updateSessionTags, setAvailableTags, availableTags } = useReticleStore()

  const handleAddTag = async () => {
    if (!newTag.trim()) return

    const tagToAdd = newTag.trim().toLowerCase()
    if (session.tags?.includes(tagToAdd)) {
      toast.error('Tag already exists')
      return
    }

    setIsLoading(true)
    try {
      await invoke('add_session_tags', {
        sessionId: session.id,
        tags: [tagToAdd],
      })

      const updatedTags = [...(session.tags || []), tagToAdd]
      updateSessionTags(session.id, updatedTags)

      // Update available tags
      if (!availableTags.includes(tagToAdd)) {
        setAvailableTags([...availableTags, tagToAdd].sort())
      }

      setNewTag('')
      setIsAdding(false)
      toast.success(`Added tag "${tagToAdd}"`)
      onTagsUpdated?.()
    } catch (error) {
      toast.error(`Failed to add tag: ${error}`)
    } finally {
      setIsLoading(false)
    }
  }

  const handleRemoveTag = async (tag: string) => {
    setIsLoading(true)
    try {
      await invoke('remove_session_tags', {
        sessionId: session.id,
        tags: [tag],
      })

      const updatedTags = (session.tags || []).filter((t) => t !== tag)
      updateSessionTags(session.id, updatedTags)
      toast.success(`Removed tag "${tag}"`)
      onTagsUpdated?.()
    } catch (error) {
      toast.error(`Failed to remove tag: ${error}`)
    } finally {
      setIsLoading(false)
    }
  }

  return (
    <div className="space-y-2">
      {/* Server name badge */}
      {session.server_name && (
        <div className="flex items-center gap-1.5">
          <Server className="w-3 h-3 text-muted-foreground" />
          <span className="text-[10px] font-mono px-1.5 py-0.5 rounded border border-border bg-muted/50">
            {session.server_name}
          </span>
        </div>
      )}

      {/* Tags */}
      <div className="flex flex-wrap items-center gap-1.5">
        <Tag className="w-3 h-3 text-muted-foreground" />
        {(session.tags || []).map((tag) => (
          <span
            key={tag}
            className="inline-flex items-center text-[10px] font-mono px-1.5 py-0 h-5 gap-1 rounded bg-secondary text-secondary-foreground"
          >
            {tag}
            <button
              onClick={() => handleRemoveTag(tag)}
              disabled={isLoading}
              className="hover:text-destructive transition-colors"
            >
              <X className="w-2.5 h-2.5" />
            </button>
          </span>
        ))}

        {isAdding ? (
          <div className="flex items-center gap-1">
            <Input
              value={newTag}
              onChange={(e) => setNewTag(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === 'Enter') handleAddTag()
                if (e.key === 'Escape') {
                  setIsAdding(false)
                  setNewTag('')
                }
              }}
              placeholder="tag name"
              className="h-5 w-20 text-[10px] px-1.5"
              autoFocus
              disabled={isLoading}
            />
            <Button
              size="sm"
              variant="ghost"
              onClick={handleAddTag}
              disabled={isLoading || !newTag.trim()}
              className="h-5 w-5 p-0"
            >
              <Plus className="w-3 h-3" />
            </Button>
            <Button
              size="sm"
              variant="ghost"
              onClick={() => {
                setIsAdding(false)
                setNewTag('')
              }}
              disabled={isLoading}
              className="h-5 w-5 p-0"
            >
              <X className="w-3 h-3" />
            </Button>
          </div>
        ) : (
          <Button
            size="sm"
            variant="ghost"
            onClick={() => setIsAdding(true)}
            className="h-5 px-1.5 text-[10px]"
          >
            <Plus className="w-3 h-3 mr-0.5" />
            Add tag
          </Button>
        )}
      </div>
    </div>
  )
}

interface ServerFilterProps {
  selectedServer: string | undefined
  onSelectServer: (server: string | undefined) => void
}

export function ServerFilter({ selectedServer, onSelectServer }: ServerFilterProps) {
  const { availableServers } = useReticleStore()

  if (availableServers.length === 0) return null

  return (
    <div className="space-y-2">
      <p className="text-[10px] text-muted-foreground uppercase tracking-wider">
        Server
      </p>
      <div className="flex flex-wrap gap-1">
        <Button
          size="sm"
          variant={!selectedServer ? 'default' : 'outline'}
          onClick={() => onSelectServer(undefined)}
          className="h-6 px-2 text-[10px]"
        >
          All
        </Button>
        {availableServers.map((server) => (
          <Button
            key={server}
            size="sm"
            variant={selectedServer === server ? 'default' : 'outline'}
            onClick={() => onSelectServer(server === selectedServer ? undefined : server)}
            className="h-6 px-2 text-[10px] font-mono"
          >
            {server}
          </Button>
        ))}
      </div>
    </div>
  )
}

interface TagFilterProps {
  selectedTags: string[]
  onSelectTags: (tags: string[]) => void
}

export function TagFilter({ selectedTags, onSelectTags }: TagFilterProps) {
  const { availableTags } = useReticleStore()

  if (availableTags.length === 0) return null

  const toggleTag = (tag: string) => {
    if (selectedTags.includes(tag)) {
      onSelectTags(selectedTags.filter((t) => t !== tag))
    } else {
      onSelectTags([...selectedTags, tag])
    }
  }

  return (
    <div className="space-y-2">
      <p className="text-[10px] text-muted-foreground uppercase tracking-wider">
        Tags
      </p>
      <div className="flex flex-wrap gap-1">
        {availableTags.map((tag) => (
          <Button
            key={tag}
            size="sm"
            variant={selectedTags.includes(tag) ? 'default' : 'outline'}
            onClick={() => toggleTag(tag)}
            className={cn(
              'h-6 px-2 text-[10px] font-mono',
              selectedTags.includes(tag) && 'bg-primary/80'
            )}
          >
            <Tag className="w-2.5 h-2.5 mr-1" />
            {tag}
          </Button>
        ))}
      </div>
    </div>
  )
}

interface QuickTagInputProps {
  sessionId: string
  /** If true, only update local store (for demo mode) */
  localOnly?: boolean
  /** If true, tags are added to active recording session (persists when recording stops) */
  isRecording?: boolean
}

export function QuickTagInput({ sessionId, localOnly = false, isRecording = false }: QuickTagInputProps) {
  const [newTag, setNewTag] = useState('')
  const [isLoading, setIsLoading] = useState(false)
  const { updateSessionTags, setAvailableTags, availableTags, sessions } = useReticleStore()

  const session = sessions.find((s) => s.id === sessionId)
  const currentTags = session?.tags || []

  const handleAddTag = async () => {
    if (!newTag.trim()) return

    const tagToAdd = newTag.trim().toLowerCase()
    if (currentTags.includes(tagToAdd)) {
      toast.error('Tag already exists')
      return
    }

    setIsLoading(true)
    try {
      // Use appropriate backend command based on mode
      if (!localOnly) {
        if (isRecording) {
          // Add tag to active recording (will persist when recording stops)
          await invoke('add_recording_tag', { tag: tagToAdd })
        } else {
          // Add tag to stored session
          await invoke('add_session_tags', {
            sessionId,
            tags: [tagToAdd],
          })
        }
      }

      const updatedTags = [...currentTags, tagToAdd]
      updateSessionTags(sessionId, updatedTags)

      if (!availableTags.includes(tagToAdd)) {
        setAvailableTags([...availableTags, tagToAdd].sort())
      }

      setNewTag('')
      toast.success(`Added tag "${tagToAdd}"`)
    } catch (error) {
      toast.error(`Failed to add tag: ${error}`)
    } finally {
      setIsLoading(false)
    }
  }

  const handleRemoveTag = async (tag: string) => {
    setIsLoading(true)
    try {
      // Use appropriate backend command based on mode
      if (!localOnly) {
        if (isRecording) {
          // Remove tag from active recording
          await invoke('remove_recording_tag', { tag })
        } else {
          // Remove tag from stored session
          await invoke('remove_session_tags', {
            sessionId,
            tags: [tag],
          })
        }
      }

      const updatedTags = currentTags.filter((t) => t !== tag)
      updateSessionTags(sessionId, updatedTags)
      toast.success(`Removed tag "${tag}"`)
    } catch (error) {
      toast.error(`Failed to remove tag: ${error}`)
    } finally {
      setIsLoading(false)
    }
  }

  return (
    <div className="space-y-2">
      <p className="text-[10px] text-muted-foreground uppercase tracking-wider flex items-center gap-1">
        <Tag className="w-3 h-3" />
        Session Tags
      </p>

      {/* Current tags */}
      {currentTags.length > 0 && (
        <div className="flex flex-wrap gap-1">
          {currentTags.map((tag) => (
            <span
              key={tag}
              className="inline-flex items-center text-[10px] font-mono px-1.5 py-0.5 rounded bg-secondary text-secondary-foreground gap-1"
            >
              {tag}
              <button
                onClick={() => handleRemoveTag(tag)}
                disabled={isLoading}
                className="hover:text-destructive transition-colors"
              >
                <X className="w-2.5 h-2.5" />
              </button>
            </span>
          ))}
        </div>
      )}

      {/* Add tag input */}
      <div className="flex items-center gap-1">
        <Input
          value={newTag}
          onChange={(e) => setNewTag(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === 'Enter') handleAddTag()
          }}
          placeholder="Add tag..."
          className="h-7 text-[10px] flex-1"
          disabled={isLoading}
        />
        <Button
          size="sm"
          variant="outline"
          onClick={handleAddTag}
          disabled={isLoading || !newTag.trim()}
          className="h-7 px-2"
        >
          <Plus className="w-3 h-3" />
        </Button>
      </div>
    </div>
  )
}
