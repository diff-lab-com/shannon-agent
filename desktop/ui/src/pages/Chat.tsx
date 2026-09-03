import { useState, useRef, useEffect, useCallback, lazy } from 'react'
import { useNavigate, useLocation } from 'react-router-dom'
import { useIntl } from 'react-intl'
import { useVirtualizer } from '@tanstack/react-virtual'
import { ArtifactProvider } from '@/components/artifact/ArtifactContext'
import { ArtifactPanel } from '@/components/artifact/ArtifactPanel'
import DiffDialog from '@/components/diff/DiffDialog'
import DiffDialogMulti from '@/components/diff/DiffDialogMulti'
import { useChat } from '@/context/ChatContext'
import { useCatalog } from '@/context/CatalogContext'
import { useSessions } from '@/context/SessionContext'
import { parseSlashInput, type SlashCommand, type SlashResult } from '@/lib/slash/commands'
import { toastError } from '@/lib/errorToast'
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
    sendMessage, contextPanelOpen,
  } = useChat()
  const { sessions, currentSessionId, createSession } = useSessions()
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
      showResult: setSlashResult,
      toastError,
      t,
    })
  }, [navigate, currentSessionId, sessions, config?.working_dir, createSession, t])

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
