// RightDock — the chat page's unified right panel stack (P0-③ / P1-⑦,
// ZCode delta ②⑦; reworked in the 2026-09 review). One resizable, tabbed
// dock modeled on ZCode's right-hand document area:
//
//   * every detected artifact (markdown / HTML / SVG / mermaid — the
//     "documents and web pages" an agent produces) gets its OWN closable
//     tab, so several files can be open side-by-side and switched freely;
//   * utility tabs (上下文 / 计划 / 预览) host the metrics, the plan doc and
//     the live dev-server preview;
//   * a "Diff" click opens a single-file review tab.
//
// Tabs auto-activate: entering plan mode docks the plan, a detected
// artifact docks it as a new tab, a "Diff" button docks the review.
// Open/close stays owned by AppContext's `contextPanelOpen` (the global
// Header toggle keeps working — that is the collapse affordance).
//
// Persisted keys:
//   shannon.dock.tab   — last active tab
//   shannon.dock.width — dock width in px (280–720, clamped to viewport)

import { useCallback, useEffect, useRef, useState } from 'react'
import { toast } from 'sonner'
import { Button } from '@/components/ui/button'
import { cn } from '@/lib/utils'
import { useT } from '@/i18n'
import { useArtifact } from '@/components/artifact/ArtifactContext'
import { artifactIcon } from '@/components/artifact/detectArtifact'
import { DocumentRenderer } from '@/components/artifact/DocumentRenderer'
import { HtmlRenderer } from '@/components/artifact/HtmlRenderer'
import { MermaidRenderer } from '@/components/artifact/MermaidRenderer'
import { SvgRenderer } from '@/components/artifact/SvgRenderer'
import DiffReviewBody from '@/components/diff/DiffReviewBody'
import type { ToolCall, UsagePayload } from '@/types'
import { ContextPanelContent } from './ContextPanel'
import PlanPanel from './PlanPanel'
import { LivePreview } from '@/components/artifact/LivePreview'

type UtilityTab = 'context' | 'plan' | 'live' | 'diff'
/** Utility tabs use bare keys; artifact tabs are `a:<artifactId>`. */
type DockTab = UtilityTab | `a:${string}`

const TAB_KEY = 'shannon.dock.tab'
const WIDTH_KEY = 'shannon.dock.width'
const MIN_WIDTH = 280
const MAX_WIDTH = 720
const DEFAULT_WIDTH = 340
const UTILITY_TABS: readonly UtilityTab[] = ['context', 'plan', 'live', 'diff']

function readTab(): DockTab {
  try {
    const raw = localStorage.getItem(TAB_KEY)
    if (raw && (UTILITY_TABS.includes(raw as UtilityTab) || raw.startsWith('a:'))) return raw as DockTab
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
  const { artifacts, setActive, close: closeArtifact } = useArtifact()
  const [tab, setTab] = useState<DockTab>(readTab)
  const [width, setWidth] = useState<number>(readWidth)
  // Q11: a one-time hint the first time the user opens the dock — they
  // learn Ctrl+\ can toggle it. Dismissed by interaction; never shown twice.
  const [hintDismissed, setHintDismissed] = useState<boolean>(
    () => typeof window !== 'undefined' && localStorage.getItem('shannon.dock.hintSeen') === '1'
  )
  const dismissHint = useCallback(() => {
    setHintDismissed(true)
    try { localStorage.setItem('shannon.dock.hintSeen', '1') } catch { /* noop */ }
  }, [])
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

  // Auto-dock: a newly detected artifact opens as its own tab and becomes
  // active (parity with the standalone panel, which appeared on detection).
  const prevArtifactCount = useRef(artifacts.length)
  useEffect(() => {
    if (artifacts.length > prevArtifactCount.current) {
      const newest = artifacts[artifacts.length - 1]
      if (newest) setTab(`a:${newest.id}`)
      onOpen()
    }
    prevArtifactCount.current = artifacts.length
  }, [artifacts, onOpen])

  // Q11: dismiss the Ctrl+\ hint once the user actively picks a tab —
  // engagement is a stronger dismissal signal than time alone.
  const handleTabPick = useCallback((next: DockTab) => {
    setTab(next)
    if (!hintDismissed) dismissHint()
  }, [hintDismissed, dismissHint])

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

  // A closed artifact tab must not stay active — fall back to context.
  useEffect(() => {
    if (tab.startsWith('a:') && !artifacts.some(a => `a:${a.id}` === tab)) {
      setTab('context')
    }
  }, [artifacts, tab])

  const activeArtifact = tab.startsWith('a:')
    ? artifacts.find(a => a.id === tab.slice(2))
    : undefined

  const utilityLabels: Record<UtilityTab, { icon: string; label: string }> = {
    context: { icon: 'data_usage', label: t('chat.dock.tab.context') },
    plan: { icon: 'route', label: t('chat.dock.tab.plan') },
    live: { icon: 'web', label: t('chat.dock.tab.live') },
    diff: { icon: 'difference', label: t('chat.dock.tab.diff') },
  }

  const tabClass = (active: boolean) =>
    cn(
      'relative shrink-0 gap-0 px-sm h-auto py-xs rounded-lg max-w-40',
      active
        ? 'bg-primary/10 text-primary hover:bg-primary/10'
        : 'text-on-surface-variant hover:bg-surface-container hover:text-on-surface',
    )

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
          {!hintDismissed && (
            <div
              role="status"
              data-testid="dock-shortcut-hint"
              className="mx-sm mt-sm flex items-center gap-sm px-sm py-xs rounded-lg bg-primary-container/40 text-on-surface text-label-xs animate-in fade-in"
            >
              <span className="material-symbols-outlined text-[14px] text-primary shrink-0">lightbulb</span>
              <span className="flex-1 min-w-0 truncate">{t('chat.dock.hint')}</span>
              <button
                type="button"
                aria-label={t('chat.dock.close.aria')}
                onClick={dismissHint}
                className="rounded p-0.5 hover:bg-surface-container text-on-surface-variant hover:text-primary"
              >
                <span className="material-symbols-outlined text-[14px]">close</span>
              </button>
            </div>
          )}
          <div role="tablist" aria-label={t('chat.dock.aria')} className="flex items-center gap-xs px-sm py-xs border-b border-outline-variant/10 shrink-0 overflow-x-auto">
            {(Object.keys(utilityLabels) as UtilityTab[]).map(key => {
              const meta = utilityLabels[key]
              const disabled = key === 'diff' && !diffPath
              return (
                <Button
                  key={key}
                  type="button"
                  variant="ghost"
                  size="sm"
                  role="tab"
                  id={`dock-tab-${key}`}
                  aria-selected={tab === key}
                  disabled={disabled}
                  onClick={() => handleTabPick(key)}
                  title={meta.label}
                  className={cn(tabClass(tab === key), disabled && 'opacity-40 pointer-events-none')}
                >
                  <span className="material-symbols-outlined icon-sm align-middle" aria-hidden="true">{meta.icon}</span>
                  <span className="align-middle ml-xs hidden md:inline">{meta.label}</span>
                </Button>
              )
            })}
            {/* One tab per open document/artifact — closable, ZCode style. */}
            {artifacts.map(a => (
              <Button
                key={a.id}
                type="button"
                variant="ghost"
                size="sm"
                role="tab"
                id={`dock-tab-a-${a.id}`}
                aria-selected={tab === `a:${a.id}`}
                onClick={() => { setActive(a.id); handleTabPick(`a:${a.id}`) }}
                title={a.title}
                className={tabClass(tab === `a:${a.id}`)}
              >
                <span className="material-symbols-outlined icon-sm align-middle" aria-hidden="true">{artifactIcon(a.kind)}</span>
                <span className="align-middle ml-xs max-w-24 truncate">{a.title}</span>
                <span
                  role="button"
                  tabIndex={0}
                  aria-label={t('chat.dock.tab.close.aria', { title: a.title })}
                  className="material-symbols-outlined icon-sm align-middle ml-xs rounded-full hover:bg-surface-container-high shrink-0"
                  onClick={e => { e.stopPropagation(); closeArtifact(a.id) }}
                  onKeyDown={e => {
                    if (e.key === 'Enter' || e.key === ' ') {
                      e.preventDefault(); e.stopPropagation(); closeArtifact(a.id)
                    }
                  }}
                >
                  close
                </span>
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
              className="text-on-surface-variant hover:text-on-surface hover:bg-surface-container shrink-0 sticky right-0"
            >
              <span className="material-symbols-outlined icon-sm">keyboard_double_arrow_right</span>
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
            {tab === 'live' && <LivePreview />}
            {tab === 'diff' && (
              diffPath ? (
                <div className="-m-lg flex flex-col min-h-0 h-full">
                  <DiffReviewBody filePath={diffPath} onClose={onCloseDiff} active={open} />
                </div>
              ) : (
                <DockEmpty icon="difference" title={t('chat.dock.empty.diff')} />
              )
            )}
            {activeArtifact && (
              <div className="-m-lg flex flex-col min-h-0 h-full">
                <ArtifactDocBody artifact={activeArtifact} />
              </div>
            )}
            {!activeArtifact && tab.startsWith('a:') && (
              <DockEmpty icon="widgets" title={t('chat.dock.empty.artifact')} />
            )}
          </div>
        </>
      )}
    </aside>
  )
}

/** Per-document body: rendered artifact + a slim copy/export/code row. */
function ArtifactDocBody({ artifact }: { artifact: { id: string; kind: string; source: string; title: string } }) {
  const t = useT()
  const [showCode, setShowCode] = useState(false)

  const handleCopy = async () => {
    try {
      await navigator.clipboard.writeText(artifact.source)
      toast.success(t('chat.artifact.copied'))
    } catch { /* clipboard unavailable — ignore */ }
  }

  return (
    <div className="flex flex-col min-h-0 flex-1">
      <div className="flex items-center gap-xs pb-sm shrink-0">
        <Button
          type="button"
          variant="ghost"
          size="sm"
          aria-pressed={showCode}
          onClick={() => setShowCode(v => !v)}
          className={cn(
            'gap-0 px-sm h-auto py-xs rounded-lg',
            showCode ? 'bg-primary/10 text-primary hover:bg-primary/10' : 'text-on-surface-variant hover:bg-surface-container hover:text-on-surface',
          )}
        >
          <span className="material-symbols-outlined icon-sm align-middle" aria-hidden="true">code</span>
          <span className="align-middle ml-xs hidden md:inline">{t('chat.artifact.tab.code')}</span>
        </Button>
        <Button
          type="button"
          variant="ghost"
          size="sm"
          onClick={handleCopy}
          className="gap-0 px-sm h-auto py-xs rounded-lg text-on-surface-variant hover:bg-surface-container hover:text-on-surface"
        >
          <span className="material-symbols-outlined icon-sm align-middle" aria-hidden="true">content_copy</span>
          <span className="align-middle ml-xs hidden md:inline">{t('chat.artifact.copy')}</span>
        </Button>
      </div>
      <div className="flex-1 min-h-0 overflow-hidden">
        {showCode ? (
          <pre className="h-full overflow-auto font-mono text-[12px] whitespace-pre-wrap break-words text-on-surface p-sm bg-surface-container-low/50 rounded-lg">
            {artifact.source}
          </pre>
        ) : artifact.kind === 'html' ? <HtmlRenderer source={artifact.source} title={artifact.title} />
          : artifact.kind === 'svg' ? <SvgRenderer source={artifact.source} title={artifact.title} />
            : artifact.kind === 'mermaid' ? <MermaidRenderer source={artifact.source} title={artifact.title} />
              : <DocumentRenderer source={artifact.source} />}
      </div>
    </div>
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
