import { useState, useRef, useEffect, useCallback, lazy } from 'react'
import { useNavigate, useLocation } from 'react-router-dom'
import { useIntl } from 'react-intl'
import { useVirtualizer } from '@tanstack/react-virtual'
import DiffDialogMulti from '@/components/diff/DiffDialogMulti'
import { useChat } from '@/context/ChatContext'
import { useCatalog } from '@/context/CatalogContext'
import { useSessions } from '@/context/SessionContext'
import { parseSlashInput, type SlashCommand, type SlashResult } from '@/lib/slash/commands'
import { toastError } from '@/lib/errorToast'
import { setActiveWorkingDir } from '@/lib/fileRefs'
import { useDiskArtifacts } from '@/hooks/useDiskArtifacts'
import { useBudgetGuard } from '@/hooks/useBudgetGuard'
import BudgetBanner from '@/components/chat/BudgetBanner'
import { TerminalPanel } from '@/components/terminal/TerminalPanel'
import RightDock from './chat/RightDock'
import {
  ApiKeyBanner,
  ComposerPanel,
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
    sendMessage, contextPanelOpen, setContextPanelOpen, compactSession,
  } = useChat()
  const { sessions, currentSessionId, createSession } = useSessions()
  const { config } = useCatalog()
  // P1-C: file-mutating tool outputs (md/html/svg/mermaid/images written to
  // disk) dock as provenance-tagged artifact tabs.
  useDiskArtifacts(messages)
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

  // Cmd/Ctrl+\ toggles the right dock — same one in AppContext the Header
  // button drives. Mirrors the terminal's Ctrl+` and keeps the keyboard
  // layer self-discoverable from the shortcuts help panel.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (!(e.metaKey || e.ctrlKey) || e.shiftKey || e.altKey) return
      if (e.key !== '\\' && e.key !== '|') return
      const el = e.target as HTMLElement | null
      if (el && (el.tagName === 'INPUT' || el.tagName === 'TEXTAREA' || el.isContentEditable)) return
      e.preventDefault()
      setContextPanelOpen(!contextPanelOpen)
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [contextPanelOpen, setContextPanelOpen])

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
    // Review 2026-09-16: scrollIntoView raced the virtualizer (the end sentinel
    // is measured before it mounts), leaving the new user bubble mid-viewport.
    // Scroll the virtualized parent directly instead.
    const el = scrollParentRef.current
    if (el) el.scrollTo({ top: el.scrollHeight, behavior: 'smooth' })
    else messagesEndRef.current?.scrollIntoView({ behavior: 'smooth' })
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

  // ── Layout (2026-09 review) ────────────────────────────────────────────
  //
  // The chat page is one full-width conversation plus the RightDock
  // (documents / diffs / live preview / context) and the terminal drawer.
  // The former workspace toolbar (对话 / Diff / 预览 preset switcher and its
  // grid panels) was retired: the preset triad read as three competing
  // "views" of the conversation and confused first-run users — everything
  // it offered now has a single home in the RightDock tabs, and the
  // terminal keeps its own drawer toggle (Ctrl+`).

  const workingDir = sessions.find(s => s.id === currentSessionId)?.working_dir
    ?? config?.working_dir
    ?? null

  // P0-B: file chips resolve relative paths against this session's working
  // dir — publish it to the module ref the chip reads (one sync per change).
  useEffect(() => {
    setActiveWorkingDir(workingDir)
  }, [workingDir])

  // P0-B: a code-file chip (non-artifact extension) opens the inline editor
  // pre-loaded with the file. Artifact extensions are handled by the
  // ArtifactLinkHost instead.
  const [editorInitialPath, setEditorInitialPath] = useState<string | null>(null)
  useEffect(() => {
    const open = (e: Event) => {
      const path = (e as CustomEvent<{ path?: string }>).detail?.path
      if (!path) return
      setEditorInitialPath(path)
      setEditorOpen(true)
    }
    window.addEventListener('shannon:open-code-file', open)
    return () => window.removeEventListener('shannon:open-code-file', open)
  }, [])

  return (
    <ComposerContext.Provider value={composerValue}>
        <div className="flex-1 flex w-full h-full relative">
          {/* Main Chat Canvas — the session list lives in the app sidebar (U1)
              and the session title + RightDock toggle live in the global
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
            panelProps={editorInitialPath ? { initialPath: editorInitialPath } : undefined}
            size="2xl"
            modalClassName="max-w-5xl h-[90vh] flex flex-col"
            bodyClassName="flex-1 overflow-hidden"
          />

          <RightDock
            open={contextPanelOpen}
            onOpen={() => setContextPanelOpen(true)}
            onClose={() => setContextPanelOpen(false)}
            usage={usage}
            activeToolCalls={activeToolCalls}
            workingDir={workingDir}
            planModeActive={config?.approval_mode === 'plan'}
            diffPath={diffPath}
            onCloseDiff={() => setDiffPath(null)}
          />
          <DiffDialogMulti open={diffPaths !== null} filePaths={diffPaths ?? []} onClose={() => setDiffPaths(null)} />
        </div>
    </ComposerContext.Provider>
  )
}
