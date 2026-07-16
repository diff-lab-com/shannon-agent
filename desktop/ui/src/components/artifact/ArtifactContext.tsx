import { createContext, useContext, useState, useCallback, useMemo, useEffect, ReactNode } from 'react'
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
  cycleNext: () => void
  autoOpen: boolean
  setAutoOpen: (v: boolean) => void
}

const ArtifactContext = createContext<ArtifactContextValue | null>(null)

const AUTO_OPEN_KEY = 'shannon.artifact.autoOpen'

let nextId = 0
function makeId(): string {
  nextId += 1
  return `a${Date.now()}_${nextId}`
}

function readAutoOpen(): boolean {
  try { return localStorage.getItem(AUTO_OPEN_KEY) === '1' } catch { return false }
}

function writeAutoOpen(v: boolean) {
  try { localStorage.setItem(AUTO_OPEN_KEY, v ? '1' : '0') } catch { /* ignore */ }
}

export function ArtifactProvider({ children }: { children: ReactNode }) {
  const [artifacts, setArtifacts] = useState<ArtifactItem[]>([])
  const [activeId, setActiveId] = useState<string | null>(null)
  const [autoOpen, setAutoOpenState] = useState<boolean>(readAutoOpen)

  const setAutoOpen = useCallback((v: boolean) => {
    setAutoOpenState(v)
    writeAutoOpen(v)
  }, [])

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

  const cycleNext = useCallback(() => {
    setArtifacts(prev => {
      if (prev.length === 0) return prev
      const idx = prev.findIndex(a => a.id === activeId)
      const nextIdx = idx < 0 ? 0 : (idx + 1) % prev.length
      setActiveId(prev[nextIdx].id)
      return prev
    })
  }, [activeId])

  useEffect(() => {
    function onKey(e: KeyboardEvent) {
      if ((e.metaKey || e.ctrlKey) && e.shiftKey && (e.key === 'A' || e.key === 'a')) {
        e.preventDefault()
        cycleNext()
      }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [cycleNext])

  const value = useMemo<ArtifactContextValue>(
    () => ({ artifacts, activeId, open, close, closeAll, setActive, cycleNext, autoOpen, setAutoOpen }),
    [artifacts, activeId, open, close, closeAll, setActive, cycleNext, autoOpen, setAutoOpen],
  )

  return <ArtifactContext.Provider value={value}>{children}</ArtifactContext.Provider>
}

export function useArtifact(): ArtifactContextValue {
  const ctx = useContext(ArtifactContext)
  if (!ctx) throw new Error('useArtifact must be used within ArtifactProvider')
  return ctx
}

