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
//   shannon.dock.width — dock width in px (280–720, clamped to 60% of the
//                        current viewport; see clampWidth)

import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { toast } from 'sonner'
import { convertFileSrc } from '@tauri-apps/api/core'
import { open as openFile, save } from '@tauri-apps/plugin-dialog'
import { Button } from '@/components/ui/button'
import { cn } from '@/lib/utils'
import { toastError } from '@/lib/errorToast'
import { useT } from '@/i18n'
import * as api from '@/lib/tauri-api'
import { openExternal, openWithDefaultApp, revealInFolder } from '@/lib/tauri-api'
import { useArtifact, type ArtifactItem } from '@/components/artifact/ArtifactContext'
import { artifactIcon, type ArtifactKind } from '@/components/artifact/detectArtifact'
import { artifactDisplayTitle, artifactKindLabel } from '@/components/artifact/labels'
import { DocumentToc } from '@/components/artifact/DocumentToc'
import { DocumentRenderer } from '@/components/artifact/DocumentRenderer'
import { HtmlRenderer } from '@/components/artifact/HtmlRenderer'
import { MermaidRenderer } from '@/components/artifact/MermaidRenderer'
import { SvgRenderer } from '@/components/artifact/SvgRenderer'
import { WebRenderer } from '@/components/artifact/WebRenderer'
import { ArtifactZoomBar, useArtifactZoom } from '@/components/artifact/ArtifactZoomBar'
import { openDiskArtifact } from '@/components/artifact/ArtifactLinkHost'
import { registerLinkPanelRouter } from '@/lib/openLink'
import { projectOf } from '@/components/SidebarSessions'
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
const FULLSCREEN_KEY = 'shannon.dock.fullscreen'
const MIN_WIDTH = 280
const MAX_WIDTH = 720
const DEFAULT_WIDTH = 340
/** B3 §P2-21: resizer keyboard step (ArrowLeft/ArrowRight). */
const RESIZE_STEP_PX = 16
const UTILITY_TABS: readonly UtilityTab[] = ['context', 'plan', 'live', 'diff']

/** Batch D4: dock fullscreen — the reading position from the dead-code
 *  ArtifactPanel, revived inside the unified dock. */
function readFullscreen(): boolean {
  try { return localStorage.getItem(FULLSCREEN_KEY) === '1' } catch { return false }
}

function readTab(): DockTab {
  try {
    const raw = localStorage.getItem(TAB_KEY)
    if (raw && (UTILITY_TABS.includes(raw as UtilityTab) || raw.startsWith('a:'))) return raw as DockTab
  } catch { /* ignore */ }
  return 'context'
}

/**
 * B3 §P2-22 width clamp: the 60%-viewport cap wins over MIN_WIDTH on
 * narrow windows (effectiveMin = min(MIN_WIDTH, innerWidth * 0.6)) and
 * over MAX_WIDTH always. Applied on drag, on restore from localStorage,
 * and on window resize — a width saved under a wide window must shrink
 * when the window is narrow, not overflow it.
 */
function clampWidth(value: number): number {
  const cap = Math.floor(window.innerWidth * 0.6)
  const max = Math.min(MAX_WIDTH, cap)
  const min = Math.min(MIN_WIDTH, cap)
  return Math.max(min, Math.min(max, value))
}

function readWidth(): number {
  try {
    const v = Number(localStorage.getItem(WIDTH_KEY))
    // Stored values re-clamp against the *current* window instead of being
    // discarded when they fall outside the static bounds.
    return Number.isFinite(v) && v > 0 ? clampWidth(v) : DEFAULT_WIDTH
  } catch {
    return DEFAULT_WIDTH
  }
}

/**
 * §P1-12: stable web-tab id — one tab per URL. Normalization is deliberately
 * minimal: trim surrounding whitespace and drop a single trailing slash
 * (`https://x/` and `https://x` are the same document). Scheme, host case,
 * query and fragment stay significant. Users who truly want a second tab of
 * the same URL get a context-menu escape hatch later (decision §5-4).
 */
function webTabId(url: string): string {
  return `web:${url.trim().replace(/\/$/, '')}`
}

/** §P2-21: the DOM id of a tab button — tabpanel aria-labelledby must point
 * at the same id, including artifact tabs (`a:<id>` → `dock-tab-a-<id>`). */
function tabDomId(key: DockTab): string {
  return key.startsWith('a:') ? `dock-tab-a-${key.slice(2)}` : `dock-tab-${key}`
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
  const { artifacts, activeId, setActive, open: openArtifact, close: closeArtifact } = useArtifact()
  const [tab, setTab] = useState<DockTab>(readTab)
  const [width, setWidth] = useState<number>(() => clampWidth(readWidth()))
  const [fullscreen, setFullscreen] = useState<boolean>(readFullscreen)
  // §P2-22: the open/close width transition must not run during a drag —
  // every setWidth would chase a 300ms ease and the panel lags the cursor
  // like a rubber band.
  const [resizing, setResizing] = useState(false)
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

  useEffect(() => {
    try { localStorage.setItem(FULLSCREEN_KEY, fullscreen ? '1' : '0') } catch { /* ignore */ }
  }, [fullscreen])

  // Closing the dock (Header Ctrl+\, collapse button) also exits fullscreen —
  // an empty fullscreen overlay must never outlive its content.
  useEffect(() => {
    if (!open && fullscreen) setFullscreen(false)
  }, [open, fullscreen])

  const onPointerMove = useCallback((e: PointerEvent) => {
    if (!draggingRef.current) return
    setWidth(clampWidth(window.innerWidth - e.clientX))
  }, [])

  const onPointerUp = useCallback(() => {
    if (draggingRef.current) {
      draggingRef.current = false
      setResizing(false)
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

  // §P2-22: re-clamp the persisted width whenever the window itself
  // resizes, so the 60% cap keeps holding at the current size.
  useEffect(() => {
    const onWindowResize = () => setWidth(w => clampWidth(w))
    window.addEventListener('resize', onWindowResize)
    return () => window.removeEventListener('resize', onWindowResize)
  }, [])

  const startResize = (e: React.PointerEvent) => {
    e.preventDefault()
    draggingRef.current = true
    setResizing(true)
    document.body.style.userSelect = 'none'
    document.body.style.cursor = 'col-resize'
  }

  // P1-E (decision §5-6): external links' `panel` target lands here — the
  // dock is the only place a web tab is visible, so the router registers
  // with this component's lifecycle (openLink degrades to the browser when
  // it is not mounted, e.g. on Settings/Welcome).
  // §P1-12: the explicit `web:<normalized-url>` id makes a second click on
  // the same URL reuse + activate its tab instead of stacking duplicates.
  useEffect(() => {
    registerLinkPanelRouter(url =>
      openArtifact({ kind: 'web', source: url, title: url, confidence: 'high', id: webTabId(url) }),
    )
    return () => registerLinkPanelRouter(null)
  }, [openArtifact])

  // Auto-dock: whenever the active artifact changes (new artifact opened
  // with activation, a file chip re-opening an already-docked file, or a
  // replace-in-place), switch the dock to its tab and reveal it. Background
  // opens (decision §5-2: disk artifacts while autoOpen is off) never touch
  // activeId, so they only add the tab.
  const prevActiveId = useRef(activeId)
  useEffect(() => {
    if (activeId && activeId !== prevActiveId.current) {
      setTab(`a:${activeId}`)
      onOpen()
    }
    prevActiveId.current = activeId
  }, [activeId, onOpen])

  // Q11: dismiss the Ctrl+\ hint once the user actively picks a tab —
  // engagement is a stronger dismissal signal than time alone.
  const handleTabPick = useCallback((next: DockTab) => {
    setTab(next)
    if (!hintDismissed) dismissHint()
  }, [hintDismissed, dismissHint])

  // §P2-21: WAI-ARIA tabs pattern — Left/Right step through tabs (skipping
  // the disabled Diff tab), Home/End jump, selection follows focus
  // (automatic activation), and only the selected tab is in the page Tab
  // order (roving tabIndex).
  const orderedTabs = useMemo<DockTab[]>(
    () => [
      ...UTILITY_TABS.filter(key => !(key === 'diff' && !diffPath)),
      ...artifacts.map(a => `a:${a.id}` as DockTab),
    ],
    [diffPath, artifacts],
  )

  const onTablistKeyDown = useCallback((e: React.KeyboardEvent) => {
    if (e.key !== 'ArrowRight' && e.key !== 'ArrowLeft' && e.key !== 'Home' && e.key !== 'End') return
    const order = orderedTabs
    if (order.length === 0) return
    e.preventDefault()
    const idx = order.indexOf(tab)
    let nextIdx: number
    if (e.key === 'Home') nextIdx = 0
    else if (e.key === 'End') nextIdx = order.length - 1
    else if (e.key === 'ArrowRight') nextIdx = (Math.max(idx, 0) + 1) % order.length
    else nextIdx = (Math.max(idx, 0) - 1 + order.length) % order.length
    const next = order[nextIdx]
    handleTabPick(next)
    document.getElementById(tabDomId(next))?.focus()
  }, [orderedTabs, tab, handleTabPick])

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

  // Batch F3, reworked in the 2026-09-25 open pipeline (§4 P1-D): the 「+」
  // opens any local file as a dock tab — documents/artifacts inline, images
  // via the asset protocol, anything else as an "open externally" card.
  // (The old implementation smuggled the content through getFileDiff's
  // old_content, which misbehaved for new/binary/oversized files.)
  const handleOpenFile = useCallback(async () => {
    try {
      const path = await openFile({ multiple: false })
      if (!path || typeof path !== 'string') return
      await openDiskArtifact(openArtifact, path, true)
    } catch (e) {
      toastError(t('chat.dock.openFile.failed'), e)
    }
  }, [openArtifact, t])

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
      className={
        fullscreen
          ? // Batch D4: fullscreen reading position — the dock covers the
            // window (above the chat, below toasts) instead of hugging it.
            'glass-panel fixed inset-0 z-modal flex flex-col overflow-hidden bg-surface-container-lowest'
          : cn(
              'glass-panel shrink-0 relative flex flex-col overflow-hidden border-l border-outline-variant/10 bg-surface-container-lowest/50',
              // §P2-22: suppress the open/close width transition while the
              // user is dragging the resizer — otherwise every pointermove
              // chases a 300ms ease and the panel lags like a rubber band.
              !resizing && 'transition-all duration-300 ease-in-out',
            )
      }
      style={
        fullscreen
          ? undefined
          : {
              width: open ? width : 0,
              borderWidth: open ? undefined : 0,
              opacity: open ? 1 : 0,
            }
      }
      inert={!open && !fullscreen}
    >
      {open && (
        <>
          {/* §P2-21: the separator is keyboard-operable — the dock hugs the
              right edge, so ArrowLeft widens and ArrowRight narrows. */}
          <div
            role="separator"
            aria-orientation="vertical"
            tabIndex={0}
            aria-label={t('chat.dock.resize.aria')}
            onPointerDown={startResize}
            onKeyDown={(e: React.KeyboardEvent) => {
              const widen = e.key === 'ArrowLeft'
              const narrow = e.key === 'ArrowRight'
              if (!widen && !narrow) return
              e.preventDefault()
              setWidth(w => clampWidth(w + (widen ? RESIZE_STEP_PX : -RESIZE_STEP_PX)))
            }}
            className="absolute left-0 top-0 bottom-0 w-1 cursor-col-resize hover:bg-primary/30 focus-visible:bg-primary/40 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-primary/40 transition-colors z-raised"
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
          {/* §P2-21: the utility buttons (+ / fullscreen / collapse) are not
              tabs — they live in a sibling of the tablist and stay on the
              same visual row via this wrapper. */}
          <div className="flex items-center px-sm py-xs border-b border-outline-variant/10 shrink-0 gap-xs">
            <div
              role="tablist"
              aria-label={t('chat.dock.aria')}
              onKeyDown={onTablistKeyDown}
              className="flex items-center gap-xs flex-1 min-w-0 overflow-x-auto"
            >
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
                    id={tabDomId(key)}
                    aria-selected={tab === key}
                    tabIndex={tab === key ? 0 : -1}
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
              {/* One tab per open document/artifact — closable, ZCode style.
                  §P2-21: the close control is a real button *sibling*
                  (absolutely positioned) with stopPropagation — an
                  interactive span nested inside the tab button violated the
                  tabs pattern and trapped assistive tech. */}
              {artifacts.map(a => {
                const active = tab === `a:${a.id}`
                return (
                  <div key={a.id} className="relative shrink-0 flex">
                    <Button
                      type="button"
                      variant="ghost"
                      size="sm"
                      role="tab"
                      id={tabDomId(`a:${a.id}`)}
                      aria-selected={active}
                      tabIndex={active ? 0 : -1}
                      onClick={() => { setActive(a.id); handleTabPick(`a:${a.id}`) }}
                      title={artifactDisplayTitle(a, t)}
                      className={cn(tabClass(active), 'pr-md')}
                    >
                      <span className="material-symbols-outlined icon-sm align-middle" aria-hidden="true">{artifactIcon(a.kind)}</span>
                      <span className="align-middle ml-xs max-w-24 truncate">{artifactDisplayTitle(a, t)}</span>
                    </Button>
                    <button
                      type="button"
                      aria-label={t('chat.dock.tab.close.aria', { title: artifactDisplayTitle(a, t) })}
                      onClick={e => { e.stopPropagation(); closeArtifact(a.id) }}
                      className="absolute right-0 top-1/2 -translate-y-1/2 p-0.5 rounded-full text-on-surface-variant hover:bg-surface-container-high hover:text-on-surface focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary/30"
                    >
                      <span className="material-symbols-outlined icon-sm" aria-hidden="true">close</span>
                    </button>
                  </div>
                )
              })}
            </div>
            <div className="flex items-center gap-xs shrink-0">
              {/* Batch F3: manual tab — open a local file as a document tab. */}
              <Button
                type="button"
                variant="ghost"
                size="icon-sm"
                onClick={() => void handleOpenFile()}
                aria-label={t('chat.dock.openFile.aria')}
                title={t('chat.dock.openFile.aria')}
                className="text-on-surface-variant hover:text-on-surface hover:bg-surface-container shrink-0"
              >
                <span className="material-symbols-outlined icon-sm">note_add</span>
              </Button>
              <Button
                type="button"
                variant="ghost"
                size="icon-sm"
                onClick={() => setFullscreen(v => !v)}
                aria-label={t(fullscreen ? 'chat.dock.fullscreenExit.aria' : 'chat.dock.fullscreen.aria')}
                title={t(fullscreen ? 'chat.dock.fullscreenExit.aria' : 'chat.dock.fullscreen.aria')}
                aria-pressed={fullscreen}
                className="text-on-surface-variant hover:text-on-surface hover:bg-surface-container shrink-0"
              >
                <span className="material-symbols-outlined icon-sm">{fullscreen ? 'fullscreen_exit' : 'fullscreen'}</span>
              </Button>
              <Button
                type="button"
                variant="ghost"
                size="icon-sm"
                onClick={onClose}
                aria-label={t('chat.dock.close.aria')}
                title={t('chat.dock.close.aria')}
                className="text-on-surface-variant hover:text-on-surface hover:bg-surface-container shrink-0"
              >
                <span className="material-symbols-outlined icon-sm">keyboard_double_arrow_right</span>
              </Button>
            </div>
          </div>
          <div
            role="tabpanel"
            id="dock-tabpanel"
            aria-labelledby={tabDomId(tab)}
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
                <ArtifactDocBody artifact={activeArtifact} workingDir={workingDir} />
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

/** Per-document body — the batch D reader: breadcrumb (项目 › 文档) +
 *  metadata row, render/code/copy/export actions, and the D1 TOC rail for
 *  multi-section documents (the ZCode 阅读器 grammar).
 *
 *  2026-09-25 open pipeline (§4 P1-D): per-kind routing — web tabs render
 *  the inline browser view, disk images render via the asset protocol, and
 *  anything without an inline renderer gets a fallback card that hands off
 *  to the OS (default app / folder reveal) instead of dead-ending. Disk
 *  artifacts (P1-C) carry a provenance badge and always expose the two OS
 *  actions. */
const DOC_FILE_EXT: Record<string, string> = {
  html: 'html',
  svg: 'svg',
  mermaid: 'mmd',
  document: 'md',
}

/** Kinds whose `source` is renderable/copyable text (code view, export…). */
const TEXT_KINDS: ReadonlySet<ArtifactKind> = new Set(['html', 'svg', 'mermaid', 'document'])

function ArtifactDocBody({ artifact, workingDir }: { artifact: ArtifactItem; workingDir: string | null }) {
  const t = useT()
  const [showCode, setShowCode] = useState(false)
  const displayTitle = artifactDisplayTitle(artifact, t)
  const project = workingDir ? projectOf({ working_dir: workingDir }) : null
  const hasText = TEXT_KINDS.has(artifact.kind)
  // `image`/`other` carry the file path in `source`; `web` carries a URL.
  const filePath = artifact.path ?? (artifact.kind === 'image' || artifact.kind === 'other' ? artifact.source : null)
  const lineCount = useMemo(() => (hasText ? artifact.source.split('\n').length : 0), [artifact.source, hasText])

  // 2026-09-26 round2 §5-1 A — chat-fence HTML (no backing file, never
  // disk-provenance) runs interactive through the artifact:// custom
  // protocol; disk files keep the static preview. Discriminator note: the
  // dock's `origin` field is only ever set to 'disk' (ArtifactLinkHost);
  // chat-fence artifacts carry no path, so `!filePath && origin !== 'disk'`
  // is exactly the chat-fence set — the same test the static hint below
  // has always used.
  const interactiveHtml = artifact.kind === 'html' && !filePath && artifact.origin !== 'disk'
  // When the interactive registration fails (web dev, oversize artifact…)
  // HtmlRenderer silently falls back to static — surface the hint again.
  const [htmlInteractiveFailed, setHtmlInteractiveFailed] = useState(false)
  useEffect(() => {
    setHtmlInteractiveFailed(false)
  }, [artifact.id, artifact.source])

  const handleCopy = async () => {
    try {
      await navigator.clipboard.writeText(artifact.source)
      toast.success(t('chat.artifact.copied'))
    } catch { /* clipboard unavailable — ignore */ }
  }

  // Batch D4: export to disk — ported from the dead-code ArtifactPanel
  // (save dialog + saveTextFile), now living where the document actually
  // renders.
  const handleExport = async () => {
    const ext = DOC_FILE_EXT[artifact.kind] ?? 'txt'
    try {
      const path = await save({
        defaultPath: `${displayTitle.replace(/[^a-zA-Z0-9-_]+/g, '_').slice(0, 60) || 'artifact'}.${ext}`,
        filters: [{ name: ext.toUpperCase(), extensions: [ext] }],
      })
      if (!path) return
      await api.saveTextFile(path, artifact.source)
      toast.success(t('chat.artifact.exported'))
    } catch (err) {
      toastError(t('chat.artifact.exportFailed'), err)
    }
  }

  const openExternally = () => {
    if (artifact.kind === 'web') {
      void openExternal(artifact.source).catch(err => toastError(t('link.open.failed'), err))
    } else if (filePath) {
      openWithDefaultApp(filePath).catch(err => toastError(t('link.open.failed'), err))
    } else if (hasText) {
      // Chat-fence artifact with no backing file (§4 P1-D): write it to
      // $TEMP and hand it to the OS default app.
      api
        .openArtifactExternally(displayTitle, artifact.source, DOC_FILE_EXT[artifact.kind] ?? 'txt')
        .catch(err => toastError(t('link.open.failed'), err))
    }
  }

  const reveal = () => {
    if (filePath) revealInFolder(filePath).catch(err => toastError(t('link.open.failed'), err))
  }

  const actionBtn =
    'gap-0 px-sm h-auto py-xs rounded-lg text-on-surface-variant hover:bg-surface-container hover:text-on-surface'

  return (
    <div className="flex flex-col min-h-0 flex-1">
      {/* D2 breadcrumb: project › document, with the metadata badges beside. */}
      <div className="flex items-center gap-xs pb-sm shrink-0 min-w-0" data-testid="artifact-doc-header">
        <span className="material-symbols-outlined icon-sm text-on-surface-variant shrink-0" aria-hidden="true">folder_open</span>
        <span className="font-label-sm text-on-surface-variant truncate" title={project ?? undefined}>
          {project ?? t('sidebar.sessions.project.untitled')}
        </span>
        <span className="material-symbols-outlined text-[13px] text-on-surface-variant shrink-0" aria-hidden="true">chevron_right</span>
        <span className="font-label-sm font-bold text-on-surface truncate flex-1 min-w-0" title={displayTitle}>
          {displayTitle}
        </span>
        {artifact.origin === 'disk' && (
          <span
            className="font-label-xs text-on-surface-variant px-xs py-[1px] rounded bg-tertiary/15 text-tertiary shrink-0"
            title={filePath ?? undefined}
          >
            {t('chat.artifact.fromDisk')}
          </span>
        )}
        <span className="font-label-xs text-on-surface-variant px-xs py-[1px] rounded bg-surface-container-high shrink-0">
          {artifactKindLabel(artifact.kind, t)}
        </span>
        {hasText && (
          <span className="font-mono text-[10px] tabular-nums text-on-surface-variant shrink-0" title={t('chat.artifact.lines.aria', { n: lineCount })}>
            {lineCount}L
          </span>
        )}
      </div>
      <div className="flex items-center gap-xs pb-sm shrink-0">
        {hasText && (
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
        )}
        {hasText && (
          <Button type="button" variant="ghost" size="sm" onClick={handleCopy} className={actionBtn}>
            <span className="material-symbols-outlined icon-sm align-middle" aria-hidden="true">content_copy</span>
            <span className="align-middle ml-xs hidden md:inline">{t('chat.artifact.copy')}</span>
          </Button>
        )}
        {hasText && (
          <Button type="button" variant="ghost" size="sm" onClick={() => void handleExport()} className={actionBtn}>
            <span className="material-symbols-outlined icon-sm align-middle" aria-hidden="true">download</span>
            <span className="align-middle ml-xs hidden md:inline">{t('chat.artifact.export')}</span>
          </Button>
        )}
        {filePath && (
          <Button type="button" variant="ghost" size="sm" onClick={reveal} className={actionBtn}>
            <span className="material-symbols-outlined icon-sm align-middle" aria-hidden="true">folder_open</span>
            <span className="align-middle ml-xs hidden md:inline">{t('chat.artifact.reveal')}</span>
          </Button>
        )}
        {(artifact.kind === 'web' || filePath || hasText) && (
          <Button type="button" variant="ghost" size="sm" onClick={openExternally} className={actionBtn}>
            <span className="material-symbols-outlined icon-sm align-middle" aria-hidden="true">open_in_new</span>
            <span className="align-middle ml-xs hidden md:inline">{t('chat.artifact.openSystem')}</span>
          </Button>
        )}
      </div>
      {/* §P1-9 / §5-1 A: the static hint now covers exactly the HTML that
          still renders statically — disk files (and a chat artifact whose
          interactive registration failed). Interactive chat-fence HTML runs
          in its own sandboxed protocol document and needs no disclaimer. */}
      {artifact.kind === 'html' && (!interactiveHtml || htmlInteractiveFailed) && (
        <div
          role="note"
          data-testid="artifact-html-static-hint"
          className="flex items-center gap-xs px-sm py-xs mb-sm rounded-lg bg-surface-container-high/50 text-on-surface-variant shrink-0"
        >
          <span className="material-symbols-outlined icon-sm shrink-0" aria-hidden="true">info</span>
          <p className="font-label-xs flex-1 min-w-0">{t('chat.dock.html.staticHint')}</p>
          <Button type="button" variant="default" size="sm" onClick={openExternally} className="shrink-0">
            <span className="material-symbols-outlined icon-sm align-middle" aria-hidden="true">open_in_new</span>
            <span className="align-middle ml-xs">{t('chat.artifact.openSystem')}</span>
          </Button>
        </div>
      )}
      <div className="flex-1 min-h-0 overflow-hidden flex gap-sm min-w-0">
        <div className="flex-1 min-w-0 min-h-0">
          {showCode && hasText ? (
            <pre className="h-full overflow-auto font-mono text-[12px] whitespace-pre-wrap break-words text-on-surface p-sm bg-surface-container-low/50 rounded-lg">
              {artifact.source}
            </pre>
          ) : artifact.kind === 'html' ? (
            <HtmlRenderer
              source={artifact.source}
              title={artifact.title}
              interactive={interactiveHtml}
              onRegistrationFailed={() => setHtmlInteractiveFailed(true)}
            />
          )
            : artifact.kind === 'web' ? <WebRenderer url={artifact.source} />
              : artifact.kind === 'image' ? <ImageDocBody src={convertFileSrc(artifact.source)} alt={displayTitle} />
                : artifact.kind === 'other' ? (
                  // P2 (§review): the dock's 「+」 may read a plain-text file
                  // that simply has no inline renderer — show its content as
                  // code instead of a dead end; only truly unreadable files
                  // get the fallback card.
                  artifact.source && artifact.source !== filePath ? (
                    <pre className="h-full overflow-auto font-mono text-[12px] whitespace-pre-wrap break-words text-on-surface p-sm bg-surface-container-low/50 rounded-lg">
                      {artifact.source}
                    </pre>
                  ) : (
                    <div className="h-full flex flex-col items-center justify-center text-center gap-xs py-xl px-lg">
                      <span className="material-symbols-outlined icon-md text-on-surface-variant/60" aria-hidden="true">draft</span>
                      <p className="font-label-md text-on-surface">{t('chat.artifact.unsupported.title')}</p>
                      <p className="font-label-sm text-on-surface-variant max-w-sm">{t('chat.artifact.unsupported.hint')}</p>
                      {filePath && (
                        <div className="flex gap-xs mt-xs">
                          <Button type="button" variant="default" size="sm" onClick={openExternally}>{t('chat.artifact.openSystem')}</Button>
                          <Button type="button" variant="ghost" size="sm" onClick={reveal}>{t('chat.artifact.reveal')}</Button>
                        </div>
                      )}
                    </div>
                  )
                )
                  : artifact.kind === 'mermaid' ? <MermaidRenderer source={artifact.source} title={artifact.title} />
                    : artifact.kind === 'svg' ? <SvgRenderer source={artifact.source} title={artifact.title} />
                      : <DocumentRenderer source={artifact.source} />}
        </div>
        {/* D1: the reader's TOC rail — multi-section documents only. */}
        {artifact.kind === 'document' && !showCode && <DocumentToc source={artifact.source} />}
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

/** B3 item 22: dock image tab with the shared zoom affordance — step
 *  buttons + Ctrl+wheel scale via CSS transform; no pan, nothing persisted. */
function ImageDocBody({ src, alt }: { src: string; alt: string }) {
  const { zoom, zoomIn, zoomOut, reset, containerRef } = useArtifactZoom()
  return (
    <div className="relative w-full h-full min-h-0">
      <div
        ref={containerRef}
        className="w-full h-full overflow-hidden bg-surface-container-low/40 rounded-lg flex items-start justify-center"
      >
        <img
          src={src}
          alt={alt}
          className="max-w-full object-contain"
          style={{ transform: `scale(${zoom})`, transformOrigin: 'top center' }}
        />
      </div>
      <ArtifactZoomBar zoom={zoom} zoomIn={zoomIn} zoomOut={zoomOut} reset={reset} className="absolute top-2 right-2 z-10" />
    </div>
  )
}
