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
import { openDiskArtifact } from '@/components/artifact/ArtifactLinkHost'
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
  const { artifacts, activeId, setActive, open: openArtifact, close: closeArtifact } = useArtifact()
  const [tab, setTab] = useState<DockTab>(readTab)
  const [width, setWidth] = useState<number>(readWidth)
  const [fullscreen, setFullscreen] = useState<boolean>(readFullscreen)
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

  // Auto-dock: an *actively* opened artifact switches the dock to its tab
  // and reveals the dock. Background opens (decision §5-2: disk artifacts
  // while autoOpen is off) only add the tab — the dock stays put.
  const prevArtifactCount = useRef(artifacts.length)
  useEffect(() => {
    if (artifacts.length > prevArtifactCount.current) {
      const newest = artifacts[artifacts.length - 1]
      if (newest && newest.id === activeId) {
        setTab(`a:${newest.id}`)
        onOpen()
      }
    }
    prevArtifactCount.current = artifacts.length
  }, [artifacts, activeId, onOpen])

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
      className={cn(
        fullscreen
          ? // Batch D4: fullscreen reading position — the dock covers the
            // window (above the chat, below toasts) instead of hugging it.
            'glass-panel fixed inset-0 z-modal flex flex-col overflow-hidden bg-surface-container-lowest'
          : 'glass-panel shrink-0 relative flex flex-col overflow-hidden border-l border-outline-variant/10 bg-surface-container-lowest/50 transition-all duration-300 ease-in-out',
      )}
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
          <div
            role="separator"
            aria-orientation="vertical"
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
                title={artifactDisplayTitle(a, t)}
                className={tabClass(tab === `a:${a.id}`)}
              >
                <span className="material-symbols-outlined icon-sm align-middle" aria-hidden="true">{artifactIcon(a.kind)}</span>
                <span className="align-middle ml-xs max-w-24 truncate">{artifactDisplayTitle(a, t)}</span>
                <span
                  role="button"
                  tabIndex={0}
                  aria-label={t('chat.dock.tab.close.aria', { title: artifactDisplayTitle(a, t) })}
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
        {(artifact.kind === 'web' || filePath) && (
          <Button type="button" variant="ghost" size="sm" onClick={openExternally} className={actionBtn}>
            <span className="material-symbols-outlined icon-sm align-middle" aria-hidden="true">open_in_new</span>
            <span className="align-middle ml-xs hidden md:inline">{t('chat.artifact.openSystem')}</span>
          </Button>
        )}
      </div>
      <div className="flex-1 min-h-0 overflow-hidden flex gap-sm min-w-0">
        <div className="flex-1 min-w-0 min-h-0">
          {showCode && hasText ? (
            <pre className="h-full overflow-auto font-mono text-[12px] whitespace-pre-wrap break-words text-on-surface p-sm bg-surface-container-low/50 rounded-lg">
              {artifact.source}
            </pre>
          ) : artifact.kind === 'html' ? <HtmlRenderer source={artifact.source} title={artifact.title} />
            : artifact.kind === 'web' ? <WebRenderer url={artifact.source} />
              : artifact.kind === 'image' ? (
                <div className="h-full w-full flex items-center justify-center bg-surface-container-low/40 rounded-lg overflow-hidden">
                  <img src={convertFileSrc(artifact.source)} alt={displayTitle} className="max-w-full max-h-full object-contain" />
                </div>
              )
                : artifact.kind === 'other' ? (
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
