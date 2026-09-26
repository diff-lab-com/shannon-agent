import { createContext, useContext, useState, useCallback, useMemo, useRef, useEffect, type ReactNode } from 'react'
import type { DetectedArtifact } from './detectArtifact'
import { stableArtifactId } from './detectArtifact'

export interface ArtifactItem extends DetectedArtifact {
  id: string
  openedAt: number
}

interface ArtifactContextValue {
  artifacts: ArtifactItem[]
  activeId: string | null
  /**
   * Dock an artifact. Artifacts carrying an explicit `id` (disk provenance
   * or a chat content hash) replace their existing tab instead of stacking
   * a duplicate; pass `activate: false` to add the tab without yanking the
   * user's attention (decision §5-2: autoOpen only controls activation).
   */
  open: (artifact: DetectedArtifact, opts?: { activate?: boolean }) => void
  close: (id: string) => void
  closeAll: () => void
  /**
   * Remove every tab with `origin === 'chat'` (§P1-14 session scoping) —
   * disk artifacts (`origin === 'disk'`) and web tabs (no origin) survive a
   * session switch. Also clears the auto-open bookkeeping, so the new
   * session's chips may auto-open again.
   */
  closeChatArtifacts: () => void
  setActive: (id: string) => void
  cycleNext: () => void
  autoOpen: boolean
  setAutoOpen: (v: boolean) => void
  /**
   * Auto-open with provider-level once-per-id bookkeeping (§P1-11). The
   * old per-chip `firedRef` reset on every virtualized remount and re-opened
   * the same artifact on each scroll cycle; this set lives as long as the
   * provider (the app window), so an id auto-opens exactly once however
   * often its chip remounts.
   */
  autoOpenOnce: (artifact: DetectedArtifact) => void
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
  // §P1-11: ids already auto-opened. A ref (not state) — marking must not
  // re-render, and the set must outlive chip mount/unmount cycles.
  const autoOpenedRef = useRef(new Set<string>())

  const setAutoOpen = useCallback((v: boolean) => {
    setAutoOpenState(v)
    writeAutoOpen(v)
  }, [])

  const open = useCallback((artifact: DetectedArtifact, opts?: { activate?: boolean }) => {
    const activate = opts?.activate ?? true
    const id = artifact.id ?? makeId()
    const item: ArtifactItem = { ...artifact, id, openedAt: Date.now() }
    setArtifacts(prev => {
      const existing = prev.findIndex(a => a.id === id)
      if (existing >= 0) {
        const next = [...prev]
        next[existing] = item
        return next
      }
      return [...prev, item]
    })
    if (activate) setActiveId(id)
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

  const closeChatArtifacts = useCallback(() => {
    setArtifacts(prev => {
      const next = prev.filter(a => a.origin !== 'chat')
      if (next.length !== prev.length) {
        // Same fallback as close(): a removed tab must not stay active —
        // fall back to the last surviving tab (or none).
        setActiveId(cur => (cur != null && next.some(a => a.id === cur) ? cur : next.length > 0 ? next[next.length - 1].id : null))
      }
      return next
    })
    // Everything in this set came from chat chips (disk artifacts bookkeep
    // in useDiskArtifacts, web tabs never auto-open), so the whole set can
    // go with the session's tabs.
    autoOpenedRef.current.clear()
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

  const autoOpenOnce = useCallback((artifact: DetectedArtifact) => {
    // Chips without an explicit id (hand-built artifacts) hash to the same
    // stable id detection would give them, so dedup still converges.
    const id = artifact.id ?? stableArtifactId(artifact.kind, artifact.source)
    if (autoOpenedRef.current.has(id)) return
    autoOpenedRef.current.add(id)
    open(artifact)
  }, [open])

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
    () => ({ artifacts, activeId, open, close, closeAll, closeChatArtifacts, setActive, cycleNext, autoOpen, setAutoOpen, autoOpenOnce }),
    [artifacts, activeId, open, close, closeAll, closeChatArtifacts, setActive, cycleNext, autoOpen, setAutoOpen, autoOpenOnce],
  )

  return <ArtifactContext.Provider value={value}>{children}</ArtifactContext.Provider>
}

export function useArtifact(): ArtifactContextValue {
  const ctx = useContext(ArtifactContext)
  if (!ctx) throw new Error('useArtifact must be used within ArtifactProvider')
  return ctx
}
