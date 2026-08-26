import { useState, useRef, useEffect, lazy } from 'react'
import { useNavigate, useLocation } from 'react-router-dom'
import { useIntl } from 'react-intl'
import { useVirtualizer } from '@tanstack/react-virtual'
import { ArtifactProvider } from '@/components/artifact/ArtifactContext'
import { ArtifactPanel } from '@/components/artifact/ArtifactPanel'
import DiffDialog from '@/components/diff/DiffDialog'
import DiffDialogMulti from '@/components/diff/DiffDialogMulti'
import { useChat } from '@/context/ChatContext'
import { useSessions } from '@/context/SessionContext'
import { useCatalog } from '@/context/CatalogContext'
import {
  ApiKeyBanner,
  ComposerPanel,
  ContextPanel,
  InlinePanelModal,
  MessageArea,
  changeSessionWorkingDir,
} from './chat'

// QuickFix and Editor are no longer top-level routes — they are inline
// tools launched from the chat input toolbar. Lazy-loaded so the main
// chat bundle stays small.
const QuickFixPanel = lazy(() => import('@/pages/QuickFix'))
const EditorPanel = lazy(() => import('@/pages/Editor'))

export default function Chat() {
  const {
    messages, streamingText, thinkingText, isQuerying, activeToolCalls, usage,
    sendMessage, cancelQuery, contextPanelOpen,
  } = useChat()
  const {
    sessions, currentSessionId,
  } = useSessions()
  const { error, config } = useCatalog()
  const intl = useIntl()
  const navigate = useNavigate()
  const location = useLocation()
  const t = (id: string) => intl.formatMessage({ id })

  const [input, setInput] = useState('')
  const [diffPath, setDiffPath] = useState<string | null>(null)
  const [diffPaths, setDiffPaths] = useState<string[] | null>(null)
  const [attachedFiles, setAttachedFiles] = useState<string[]>([])
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

  // Virtualization only kicks in past the threshold. Below it, the overhead
  // of measuring/positioning outweighs the win from fewer DOM nodes — and
  // jsdom can't provide real dimensions, so tests would render zero items.
  const shouldVirtualize = messages.length > 30

  useEffect(() => {
    messagesEndRef.current?.scrollIntoView({ behavior: 'smooth' })
  }, [messages, streamingText])

  // C2: Cmd/Ctrl+D triggers the WD picker from anywhere. The handler is
  // defined later in the component but stable per-render — we always
  // dispatch through a ref so the listener doesn't need to re-bind.
  const changeWorkingDirRef = useRef<() => void>(() => {})
  useEffect(() => {
    const handler = () => changeWorkingDirRef.current()
    window.addEventListener('shannon:change-wd', handler)
    return () => window.removeEventListener('shannon:change-wd', handler)
  }, [])

  const handleSend = () => {
    const trimmed = input.trim()
    if (!trimmed || isQuerying) return
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

  const currentSession = sessions.find(s => s.id === currentSessionId)
  const sessionWorkingDir = currentSession?.working_dir ?? config?.working_dir ?? ''

  const showApiKeyBanner =
    !bannerDismissed &&
    !!config &&
    !config.api_key &&
    config.provider !== 'ollama'

  const handleChangeWorkingDir = () => changeSessionWorkingDir(currentSessionId, t)
  changeWorkingDirRef.current = handleChangeWorkingDir

  return (
    <ArtifactProvider>
      <div className="flex-1 flex w-full h-full relative">
        {/* Main Chat Canvas — the session list lives in the app sidebar (U1)
            and the session title + ContextPanel toggle live in the global
            Header (U2); the ChatHeader bar is retired. */}
        <section className="flex-1 flex flex-col relative bg-surface-container-lowest/40 overflow-hidden">
          <ApiKeyBanner
            t={t}
            visible={showApiKeyBanner}
            onDismiss={() => setBannerDismissed(true)}
            onOpenSettings={() => navigate('/settings/models')}
          />

          <MessageArea
            t={t}
            scrollParentRef={scrollParentRef}
            messagesEndRef={messagesEndRef}
            messages={messages}
            streamingText={streamingText}
            thinkingText={thinkingText}
            activeToolCalls={activeToolCalls}
            error={error}
            virtualizer={virtualizer}
            shouldVirtualize={shouldVirtualize}
            setInput={setInput}
            handleSend={handleSend}
            setDiffPath={setDiffPath}
            setDiffPaths={setDiffPaths}
            input={input}
          />

          <ComposerPanel
            t={t}
            input={input}
            setInput={setInput}
            handleSend={handleSend}
            attachedFiles={attachedFiles}
            handleAttach={handleAttach}
            handleDetachAll={handleDetachAll}
            isQuerying={isQuerying}
            cancelQuery={cancelQuery}
            currentSessionId={currentSessionId}
            sessionWorkingDir={sessionWorkingDir}
            handleChangeWorkingDir={handleChangeWorkingDir}
            setQuickFixOpen={setQuickFixOpen}
            setEditorOpen={setEditorOpen}
          />
        </section>

        <InlinePanelModal
          t={t}
          open={quickFixOpen}
          onClose={() => setQuickFixOpen(false)}
          title={t('nav.quickFix')}
          panel={QuickFixPanel}
          size="2xl"
          modalClassName="max-w-3xl max-h-[85vh] overflow-y-auto"
          bodyClassName="p-lg"
        />

        <InlinePanelModal
          t={t}
          open={editorOpen}
          onClose={() => setEditorOpen(false)}
          title={t('nav.editor')}
          panel={EditorPanel}
          size="2xl"
          modalClassName="max-w-5xl h-[90vh] flex flex-col"
          bodyClassName="flex-1 overflow-hidden"
        />

        <ContextPanel
          t={t}
          open={contextPanelOpen}
          usage={usage}
          activeToolCalls={activeToolCalls}
        />
        <DiffDialog open={diffPath !== null} filePath={diffPath} onClose={() => setDiffPath(null)} />
        <DiffDialogMulti open={diffPaths !== null} filePaths={diffPaths ?? []} onClose={() => setDiffPaths(null)} />
        <ArtifactPanel />
      </div>
    </ArtifactProvider>
  )
}