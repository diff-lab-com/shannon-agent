// RightDock — the chat page's unified right panel stack (P0-③ / P1-⑦,
// ZCode delta ②⑦). One resizable, tabbed dock hosting what used to be three
// separate surfaces: the context/metrics panel, the artifact panel, the
// session's plan document, and single-file diff review. Tabs auto-activate:
// entering plan mode docks the plan, a detected artifact docks the artifact,
// a "Diff" button docks the review. Open/close stays owned by AppContext's
// `contextPanelOpen` (the global Header toggle keeps working).
//
// Persisted keys:
//   shannon.dock.tab   — last active tab
//   shannon.dock.width — dock width in px (280–720, clamped to viewport)

import { useCallback, useEffect, useRef, useState } from 'react'
import { Button } from '@/components/ui/button'
import { cn } from '@/lib/utils'
import { useT } from '@/i18n'
import { useArtifact } from '@/components/artifact/ArtifactContext'
import { ArtifactPanel } from '@/components/artifact/ArtifactPanel'
import DiffReviewBody from '@/components/diff/DiffReviewBody'
import type { ToolCall, UsagePayload } from '@/types'
import { ContextPanelContent } from './ContextPanel'
import PlanPanel from './PlanPanel'

export type DockTab = 'context' | 'plan' | 'artifact' | 'diff'

const TAB_KEY = 'shannon.dock.tab'
const WIDTH_KEY = 'shannon.dock.width'
const MIN_WIDTH = 280
const MAX_WIDTH = 720
const DEFAULT_WIDTH = 340

function readTab(): DockTab {
  try {
    const raw = localStorage.getItem(TAB_KEY)
    if (raw === 'plan' || raw === 'artifact' || raw === 'diff' || raw === 'context') return raw
  } catch { /* ignore */ }
  return 'context'
}

function readWidth(): number {
  try {
    const v = Number(localStorage.getItem(WIDTH_KEY))
    return Number.isFinite(v) && v >= MIN_WIDTH && v <= MAX_WIDTH ? v : DEFAULT_WIDTH
  } catch { return DEFAULT_WIDTH }
}

interface RightDockProps {
  open: boolean
  onOpen: () => void
  onClose: () => void
  usage: UsagePayload | null
  activeToolCalls: ToolCall[]
  /** Session working dir — feeds the plan tab. */
  workingDir: string | null
  /** Composer plan-mode chip state — entering plan mode docks the plan tab. */
  planModeActive: boolean
  /** Single-file diff review target; null closes the diff tab. */
  diffPath: string | null
  onCloseDiff: () => void
}

export default function RightDock({
  open,
  onOpen,
  onClose,
  usage,
  activeToolCalls,
  workingDir,
  planModeActive,
  diffPath,
  onCloseDiff,
}: RightDockProps) {
  const t = useT()
  const { artifacts } = useArtifact()
  const [tab, setTab] = useState<DockTab>(readTab)
  const [width, setWidth] = useState<number>(readWidth)
  const draggingRef = useRef(false)

  useEffect(() => {
    try { localStorage.setItem(TAB_KEY, tab) } catch { /* ignore */ }
  }, [tab])

  useEffect(() => {
    try { localStorage.setItem(WIDTH_KEY, String(width)) } catch { /* ignore */ }
  }, [width])

  const onPointerMove = useCallback((e: PointerEvent) => {
    if (!draggingRef.current) return
    const raw = window.innerWidth - e.clientX
    setWidth(Math.max(MIN_WIDTH, Math.min(MAX_WIDTH, raw, Math.floor(window.innerWidth * 0.6))))
  }, [])

  const onPointerUp = useCallback(() => {
    if (draggingRef.current) {
      draggingRef.current = false
      document.body.style.userSelect = ''
      document.body.style.cursor = ''
    }
  }, [])

  useEffect(() => {
    window.addEventListener('pointermove', onPointerMove)
    window.addEventListener('pointerup', onPointerUp)
    return () => {
      window.removeEventListener('pointermove', onPointerMove)
      window.removeEventListener('pointerup', onPointerUp)
    }
  }, [onPointerMove, onPointerUp])

  const startResize = (e: React.PointerEvent) => {
    e.preventDefault()
    draggingRef.current = true
    document.body.style.userSelect = 'none'
    document.body.style.cursor = 'col-resize'
  }

  // Auto-dock: a newly detected artifact switches to the artifact tab
  // (parity with the standalone panel, which appeared on detection).
  const prevArtifactCount = useRef(artifacts.length)
  useEffect(() => {
    if (artifacts.length > prevArtifactCount.current) {
      setTab('artifact')
      onOpen()
    }
    prevArtifactCount.current = artifacts.length
  }, [artifacts.length, onOpen])

  // Entering plan mode docks the plan document (ZCode's 计划 tab behavior).
  const prevPlanMode = useRef(planModeActive)
  useEffect(() => {
    if (planModeActive && !prevPlanMode.current) {
      setTab('plan')
      onOpen()
    }
    prevPlanMode.current = planModeActive
  }, [planModeActive, onOpen])

  // A "Diff" click docks the single-file review instead of a modal.
  const prevDiffPath = useRef(diffPath)
  useEffect(() => {
    if (diffPath && diffPath !== prevDiffPath.current) {
      setTab('diff')
      onOpen()
    }
    prevDiffPath.current = diffPath
  }, [diffPath, onOpen])

  const tabs: { key: DockTab; icon: string; label: string; badge?: boolean }[] = [
    { key: 'context', icon: 'data_usage', label: t('chat.dock.tab.context') },
    { key: 'plan', icon: 'route', label: t('chat.dock.tab.plan') },
    { key: 'artifact', icon: 'widgets', label: t('chat.dock.tab.artifact'), badge: artifacts.length > 0 },
    ...(diffPath ? [{ key: 'diff' as const, icon: 'difference', label: t('chat.dock.tab.diff') }] : []),
  ]

  return (
    <aside
      aria-label={t('chat.dock.aria')}
      // Keyboard-scrollable region (axe scrollable-region-focusable).
      tabIndex={0}
      className="glass-panel shrink-0 relative flex flex-col overflow-hidden border-l border-outline-variant/10 bg-surface-container-lowest/50 transition-all duration-300 ease-in-out"
      style={{
        width: open ? width : 0,
        borderWidth: open ? undefined : 0,
        opacity: open ? 1 : 0,
      }}
      inert={!open}
    >
      {open && (
        <>
          <div
            onPointerDown={startResize}
            className="absolute left-0 top-0 bottom-0 w-1 cursor-col-resize hover:bg-primary/30 transition-colors z-raised"
            aria-label={t('chat.dock.resize.aria')}
          />
          <div role="tablist" aria-label={t('chat.dock.aria')} className="flex items-center gap-xs px-sm py-xs border-b border-outline-variant/10 shrink-0">
            {tabs.map(item => (
              <Button
                key={item.key}
                type="button"
                variant="ghost"
                size="sm"
                role="tab"
                id={`dock-tab-${item.key}`}
                aria-selected={tab === item.key}
                onClick={() => setTab(item.key)}
                title={item.label}
                className={cn(
                  'relative shrink-0 gap-0 px-sm h-auto py-xs rounded-lg',
                  tab === item.key
                    ? 'bg-primary/10 text-primary hover:bg-primary/10'
                    : 'text-on-surface-variant hover:bg-surface-container hover:text-on-surface',
                )}
              >
                <span className="material-symbols-outlined icon-sm align-middle" aria-hidden="true">{item.icon}</span>
                <span className="align-middle ml-xs hidden lg:inline">{item.label}</span>
                {item.badge && tab !== item.key && (
                  <span aria-hidden="true" className="absolute top-0.5 right-0.5 w-1.5 h-1.5 rounded-full bg-secondary" />
                )}
              </Button>
            ))}
            <div className="flex-1" />
            <Button
              type="button"
              variant="ghost"
              size="icon-sm"
              onClick={onClose}
              aria-label={t('chat.dock.close.aria')}
              title={t('chat.dock.close.aria')}
              className="text-on-surface-variant hover:text-on-surface hover:bg-surface-container shrink-0"
            >
              <span className="material-symbols-outlined icon-sm">close</span>
            </Button>
          </div>
          <div
            role="tabpanel"
            id="dock-tabpanel"
            aria-labelledby={`dock-tab-${tab}`}
            className="flex-1 min-h-0 overflow-y-auto p-lg"
          >
            {tab === 'context' && <ContextPanelContent usage={usage} activeToolCalls={activeToolCalls} />}
            {tab === 'plan' && <PlanPanel workingDir={workingDir} planModeActive={planModeActive} />}
            {tab === 'artifact' && (
              artifacts.length > 0 ? (
                <div className="-m-lg">
                  <ArtifactPanel embedded />
                </div>
              ) : (
                <DockEmpty icon="widgets" title={t('chat.dock.empty.artifact')} />
              )
            )}
            {tab === 'diff' && (
              diffPath ? (
                <div className="-m-lg flex flex-col min-h-0 h-full">
                  <DiffReviewBody filePath={diffPath} onClose={onCloseDiff} active={open} />
                </div>
              ) : (
                <DockEmpty icon="difference" title={t('chat.dock.empty.diff')} />
              )
            )}
          </div>
        </>
      )}
    </aside>
  )
}

function DockEmpty({ icon, title }: { icon: string; title: string }) {
  return (
    <div className="h-full flex flex-col items-center justify-center text-center gap-xs py-xl">
      <span className="material-symbols-outlined icon-md text-on-surface-variant/60" aria-hidden="true">{icon}</span>
      <p className="font-label-md text-on-surface-variant">{title}</p>
    </div>
  )
}
