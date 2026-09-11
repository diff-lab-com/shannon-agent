import { useState, useRef, useEffect, useLayoutEffect, useCallback, lazy } from 'react'
import { useNavigate, useLocation } from 'react-router-dom'
import { useIntl } from 'react-intl'
import { useVirtualizer } from '@tanstack/react-virtual'
import { ArtifactProvider } from '@/components/artifact/ArtifactContext'
import { ArtifactPanel } from '@/components/artifact/ArtifactPanel'
import { LivePreview } from '@/components/artifact/LivePreview'
import DiffDialog from '@/components/diff/DiffDialog'
import DiffDialogMulti from '@/components/diff/DiffDialogMulti'
import { useChat } from '@/context/ChatContext'
import { useCatalog } from '@/context/CatalogContext'
import { useSessions } from '@/context/SessionContext'
import { parseSlashInput, type SlashCommand, type SlashResult } from '@/lib/slash/commands'
import { toastError } from '@/lib/errorToast'
import { useBudgetGuard } from '@/hooks/useBudgetGuard'
import BudgetBanner from '@/components/chat/BudgetBanner'
import { TerminalPanel } from '@/components/terminal/TerminalPanel'
import {
  addPanel,
  matchesPreset,
  presetLayout,
  workspaceProjectKey,
  type PanelKind,
} from '@/components/workspace/layout'
import { useWorkspaceLayout } from '@/components/workspace/useWorkspaceLayout'
import { WorkspaceGrid } from '@/components/workspace/WorkspaceGrid'
import { WorkspaceToolbar } from '@/components/workspace/WorkspaceToolbar'
import { SessionDiffPanel } from '@/components/workspace/SessionDiffPanel'
import {
  ApiKeyBanner,
  ComposerPanel,
  ContextPanel,
  InlinePanelModal,
  MessageArea,
  ComposerContext,
} from './chat'

// QuickFix is a chat-inline tool launched from the composer toolbar (it has
// no standalone route); the Editor exists both inline and as a standalone
// route (palette / mod+5). Lazy-loaded so the main chat bundle stays small.
const QuickFixPanel = lazy(() => import('@/pages/QuickFix'))
const EditorPanel = lazy(() => import('@/pages/Editor'))

export default function Chat() {
  const {
    messages, streamingText, isQuerying, usage, activeToolCalls,
    sendMessage, contextPanelOpen, compactSession,
  } = useChat()
  const { sessions, currentSessionId, windowSessionId, createSession } = useSessions()
  const { config } = useCatalog()
  const intl = useIntl()
  const t = useCallback((id: string) => intl.formatMessage({ id }), [intl])
  const navigate = useNavigate()
  const location = useLocation()

  // Composer state (draft + attachments) is provided through the page-local
  // ComposerContext — MessageArea / ComposerPanel consume it directly instead
  // of receiving it (plus handlers) through two layers of props.
  const [input, setInput] = useState('')
  const [attachedFiles, setAttachedFiles] = useState<string[]>([])
  const [diffPath, setDiffPath] = useState<string | null>(null)
  const [diffPaths, setDiffPaths] = useState<string[] | null>(null)
  const [quickFixOpen, setQuickFixOpen] = useState(false)
  const [editorOpen, setEditorOpen] = useState(false)
  // Standalone /editor route retired — external entry points (mod+5, command
  // palette, /editor slash) open the chat-inline panel via this event.
  useEffect(() => {
    const open = () => setEditorOpen(true)
    window.addEventListener('shannon:open-editor', open)
    return () => window.removeEventListener('shannon:open-editor', open)
  }, [])

  // Pre-fill the composer when navigated from elsewhere (e.g. Editor's
  // "Ask AI about this diagnostic" button passes { prefill } in location.state).
  // Guard with a ref so the effect doesn't re-fire on every keystroke that
  // updates `input` — only react to the navigation event itself.
  const prefillApplied = useRef(false)
  useEffect(() => {
    if (prefillApplied.current) return
    const prefill = (location.state as { prefill?: string } | null)?.prefill
    if (prefill) {
      setInput(prefill)
      prefillApplied.current = true
      navigate(location.pathname, { replace: true, state: null })
    }
  }, [location.state, location.pathname, navigate])

  const [bannerDismissed, setBannerDismissed] = useState(false)

  // P0-4: session-budget advisory/choice bars. "Continue once" resends the
  // last user message with the budget-bypass flag (exempts exactly that
  // send's pre-turn check backend-side).
  const budgetGuard = useBudgetGuard(currentSessionId)
  const continuePastBudget = useCallback(() => {
    const lastUser = [...messages].reverse().find(m => m.role === 'user')
    if (lastUser) void sendMessage(lastUser.content, undefined, { budgetBypass: true })
  }, [messages, sendMessage])
  const messagesEndRef = useRef<HTMLDivElement>(null)
  const scrollParentRef = useRef<HTMLDivElement>(null)

  const virtualizer = useVirtualizer({
    count: messages.length,
    getScrollElement: () => scrollParentRef.current,
    estimateSize: () => 200,
    overscan: 4,
    measureElement: typeof window !== 'undefined' && 'ResizeObserver' in window
      ? (el) => el.getBoundingClientRect().height
      : undefined,
  })

  useEffect(() => {
    messagesEndRef.current?.scrollIntoView({ behavior: 'smooth' })
  }, [messages, streamingText])

  const [slashResult, setSlashResult] = useState<SlashResult | null>(null)
  const dismissSlashResult = useCallback(() => setSlashResult(null), [])

  const executeSlash = useCallback((cmd: SlashCommand) => {
    void cmd.run({
      navigate,
      sessionId: currentSessionId,
      workingDir: sessions.find(s => s.id === currentSessionId)?.working_dir
        ?? config?.working_dir
        ?? '',
      sessions,
      createSession,
      compactSession,
      showResult: setSlashResult,
      toastError,
      t,
    })
  }, [navigate, currentSessionId, sessions, config?.working_dir, createSession, compactSession, t])

  const handleSend = () => {
    const trimmed = input.trim()
    if (!trimmed || isQuerying) return
    // Slash commands never reach the model: a bare `/name` runs locally
    // (the desktop's counterpart of the REPL command line), and anything
    // else starting with `/` — typically a pasted absolute path — is sent
    // as plain text.
    const slashCommand = parseSlashInput(trimmed)
    if (slashCommand) {
      executeSlash(slashCommand)
      setInput('')
      return
    }
    const filePaths = attachedFiles.length > 0 ? attachedFiles : undefined
    sendMessage(trimmed, filePaths)
    setInput('')
    setAttachedFiles([])
  }

  // Attach files via Tauri's native dialog so the backend receives real
  // absolute paths (the backend reads bytes via std::fs and base64-encodes).
  // The browser <input type="file"> only exposes File objects with opaque
  // "fakepath" paths, which never resolve on disk — that was the dead-button bug.
  const handleAttach = async (files: string[]) => {
    if (files.length > 0) setAttachedFiles(prev => [...prev, ...files])
  }

  const handleDetachAll = () => {
    setAttachedFiles([])
  }

  const composerValue = {
    input, setInput, handleSend,
    attachedFiles, handleAttach, handleDetachAll,
    executeSlash, slashResult, dismissSlashResult,
  }

  const showApiKeyBanner =
    !bannerDismissed &&
    !!config &&
    !config.api_key &&
    config.provider !== 'ollama'

  // ── P1-5 C-2 — workspace host ─────────────────────────────────────────
  //
  // Window mode (windowSessionId) keeps the slim single-chat view with no
  // workspace at all (brief: 仅显示 chat 面板，禁用布局编辑). The main
  // window hosts the WorkspaceGrid; the default focus preset renders the
  // chat panel chrome-less over the full grid, so the default appearance
  // matches the pre-workspace page.

  const windowMode = windowSessionId != null
  const workingDir = sessions.find(s => s.id === currentSessionId)?.working_dir
    ?? config?.working_dir
    ?? null
  const { layout, update } = useWorkspaceLayout(windowMode ? null : (workingDir ? workspaceProjectKey(workingDir) : null))
  const hasTerminalPanel = layout.panels.some(p => p.kind === 'terminal')
  const showChrome = !matchesPreset(layout, 'focus')

  // The terminal drawer and the terminal grid panel are the SAME
  // TerminalPanel instance (see TerminalPanelProps.variant). React 19
  // UNMOUNTS and remounts portal children when a portal's container prop
  // changes — which would dispose every xterm instance — so instead the
  // panel is rendered ONCE into a parked wrapper and a layout effect
  // physically MOVES that wrapper DOM node between the two slot containers
  // (manual reparenting never remounts React-managed nodes). The xterm
  // instances — including scrollback — survive the handoff, and the
  // Rust-side TerminalManager keeps the PTY processes alive regardless
  // (never re-spawned). The embedded variant additionally reconciles with
  // `terminal_list` on mount as defense in depth.
  const [drawerSlot, setDrawerSlot] = useState<HTMLDivElement | null>(null)
  const [gridTerminalSlot, setGridTerminalSlot] = useState<HTMLDivElement | null>(null)
  const terminalDockRef = useRef<HTMLDivElement | null>(null)

  useLayoutEffect(() => {
    const dock = terminalDockRef.current
    const target = hasTerminalPanel ? gridTerminalSlot : drawerSlot
    if (dock && target && dock.parentElement !== target) {
      // Physical reparent — preserves the mounted TerminalPanel subtree.
      target.appendChild(dock)
    }
  }, [hasTerminalPanel, gridTerminalSlot, drawerSlot])

  const renderPanelContent = useCallback((kind: PanelKind) => {
    switch (kind) {
      case 'chat':
        return (
          <>
            <MessageArea
              scrollParentRef={scrollParentRef}
              messagesEndRef={messagesEndRef}
              virtualizer={virtualizer}
              setDiffPath={setDiffPath}
              setDiffPaths={setDiffPaths}
            />
            <ComposerPanel
              setQuickFixOpen={setQuickFixOpen}
              setEditorOpen={setEditorOpen}
            />
            {/* Terminal drawer slot: hosts the docked TerminalPanel while
                the layout has no terminal grid panel. */}
            <div
              ref={setDrawerSlot}
              className={hasTerminalPanel ? 'hidden' : 'contents'}
            />
          </>
        )
      case 'diff':
        return <SessionDiffPanel workingDir={workingDir} />
      case 'preview':
        return <LivePreview />
      case 'terminal':
        return (
          <div
            ref={setGridTerminalSlot}
            data-testid="workspace-terminal-slot"
            className="h-full min-h-0"
          />
        )
    }
  }, [virtualizer, workingDir, hasTerminalPanel])

  return (
    <ArtifactProvider>
      <ComposerContext.Provider value={composerValue}>
        <div className="flex-1 flex w-full h-full relative">
          {/* Main Chat Canvas — the session list lives in the app sidebar (U1)
              and the session title + ContextPanel toggle live in the global
              Header (U2); the ChatHeader bar is retired. */}
          <section className="flex-1 flex flex-col relative bg-surface-container-lowest/40 overflow-hidden">
            <ApiKeyBanner
              visible={showApiKeyBanner}
              onDismiss={() => setBannerDismissed(true)}
              onOpenSettings={() => navigate('/settings/models')}
            />

            <BudgetBanner
              warning={budgetGuard.warning}
              exceeded={budgetGuard.exceeded}
              clearWarning={budgetGuard.clearWarning}
              clearExceeded={budgetGuard.clearExceeded}
              onContinueOnce={continuePastBudget}
              sessionId={currentSessionId}
            />

            {windowMode ? (
              <>
                {/* P1-1 window mode: slim single-chat view, no workspace. */}
                <MessageArea
                  scrollParentRef={scrollParentRef}
                  messagesEndRef={messagesEndRef}
                  virtualizer={virtualizer}
                  setDiffPath={setDiffPath}
                  setDiffPaths={setDiffPaths}
                />
                <ComposerPanel
                  setQuickFixOpen={setQuickFixOpen}
                  setEditorOpen={setEditorOpen}
                />
                <TerminalPanel projectDir={workingDir} />
              </>
            ) : (
              <>
                <WorkspaceToolbar
                  layout={layout}
                  onPreset={name => update(presetLayout(name))}
                  onAdd={kind => update(addPanel(layout, kind))}
                  onReset={() => update(presetLayout('focus'))}
                />
                <div className="flex-1 min-h-0">
                  <WorkspaceGrid
                    layout={layout}
                    chrome={showChrome}
                    onChange={update}
                    renderPanelContent={renderPanelContent}
                    ariaLabel={t('workspace.grid.aria')}
                  />
                </div>
                {/* One docked TerminalPanel instance. It renders here only
                    until the layout effect moves the dock node into the
                    active slot (drawer slot inside the chat panel, or the
                    terminal grid panel's slot) — the move never remounts it. */}
                <div ref={terminalDockRef} data-testid="terminal-dock" className="contents">
                  <TerminalPanel
                    projectDir={workingDir}
                    variant={hasTerminalPanel ? 'panel' : 'drawer'}
                  />
                </div>
              </>
            )}
          </section>

          <InlinePanelModal
            open={quickFixOpen}
            onClose={() => setQuickFixOpen(false)}
            title={t('nav.quickFix')}
            panel={QuickFixPanel}
            size="2xl"
            modalClassName="max-w-3xl max-h-[85vh] overflow-y-auto"
            bodyClassName="p-lg"
          />

          <InlinePanelModal
            open={editorOpen}
            onClose={() => setEditorOpen(false)}
            title={t('nav.editor')}
            panel={EditorPanel}
            size="2xl"
            modalClassName="max-w-5xl h-[90vh] flex flex-col"
            bodyClassName="flex-1 overflow-hidden"
          />

          <ContextPanel
            open={contextPanelOpen}
            usage={usage}
            activeToolCalls={activeToolCalls}
          />
          <DiffDialog open={diffPath !== null} filePath={diffPath} onClose={() => setDiffPath(null)} />
          <DiffDialogMulti open={diffPaths !== null} filePaths={diffPaths ?? []} onClose={() => setDiffPaths(null)} />
          <ArtifactPanel />
        </div>
      </ComposerContext.Provider>
    </ArtifactProvider>
  )
}
