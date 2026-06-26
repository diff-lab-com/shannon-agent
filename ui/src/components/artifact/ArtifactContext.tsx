import { createContext, useContext, useState, useCallback, useMemo, ReactNode } from 'react'
import type { DetectedArtifact } from './detectArtifact'

export interface ArtifactItem extends DetectedArtifact {
  id: string
  openedAt: number
}

interface ArtifactContextValue {
  artifacts: ArtifactItem[]
  activeId: string | null
  open: (artifact: DetectedArtifact) => void
  close: (id: string) => void
  closeAll: () => void
  setActive: (id: string) => void
}

const ArtifactContext = createContext<ArtifactContextValue | null>(null)

let nextId = 0
function makeId(): string {
  nextId += 1
  return `a${Date.now()}_${nextId}`
}

export function ArtifactProvider({ children }: { children: ReactNode }) {
  const [artifacts, setArtifacts] = useState<ArtifactItem[]>([])
  const [activeId, setActiveId] = useState<string | null>(null)

  const open = useCallback((artifact: DetectedArtifact) => {
    const id = makeId()
    setArtifacts(prev => [...prev, { ...artifact, id, openedAt: Date.now() }])
    setActiveId(id)
  }, [])

  const close = useCallback((id: string) => {
    setArtifacts(prev => {
      const next = prev.filter(a => a.id !== id)
      if (id === activeId) {
        setActiveId(next.length > 0 ? next[next.length - 1].id : null)
      }
      return next
    })
  }, [activeId])

  const closeAll = useCallback(() => {
    setArtifacts([])
    setActiveId(null)
  }, [])

  const setActive = useCallback((id: string) => setActiveId(id), [])

  const value = useMemo<ArtifactContextValue>(
    () => ({ artifacts, activeId, open, close, closeAll, setActive }),
    [artifacts, activeId, open, close, closeAll, setActive],
  )

  return <ArtifactContext.Provider value={value}>{children}</ArtifactContext.Provider>
}

export function useArtifact(): ArtifactContextValue {
  const ctx = useContext(ArtifactContext)
  if (!ctx) throw new Error('useArtifact must be used within ArtifactProvider')
  return ctx
}
