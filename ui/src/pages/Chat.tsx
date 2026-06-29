import { useState, useRef, useEffect, useMemo, memo, lazy, Suspense } from 'react'
import { useNavigate, useLocation } from 'react-router-dom'
import { useIntl } from 'react-intl'
import { toast } from 'sonner'
import { open as openDialog, save as saveDialog } from '@tauri-apps/plugin-dialog'
import { useVirtualizer } from '@tanstack/react-virtual'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { ScrollArea } from '@/components/ui/scroll-area'
import { Pagination } from '@/components/ui/pagination'
import { Banner } from '@/components/ui/banner'
import WelcomeState from '@/components/WelcomeState'
import DiffDialog from '@/components/diff/DiffDialog'
import DiffDialogMulti from '@/components/diff/DiffDialogMulti'
import { ArtifactProvider } from '@/components/artifact/ArtifactContext'
import { ArtifactPanel } from '@/components/artifact/ArtifactPanel'
import ChatInput from '@/components/chat/ChatInput'
import { MessageBubble } from '@/components/chat/MessageBubble'
import StreamingResponse from '@/components/chat/StreamingResponse'
import { useChat } from '@/context/ChatContext'
import { useSessions } from '@/context/SessionContext'
import { useCatalog } from '@/context/CatalogContext'
import { useModalFocus } from '@/hooks/useModalFocus'
import * as api from '@/lib/tauri-api'
import { buildPrintStyles } from '@/lib/printStyles'
import type { SessionInfo } from '@/types'

// QuickFix and Editor are no longer top-level routes — they are inline
// tools launched from the chat input toolbar. Lazy-loaded so the main
// chat bundle stays small.
const QuickFixPanel = lazy(() => import('@/pages/QuickFix'))
const EditorPanel = lazy(() => import('@/pages/Editor'))

// Render a tiny subset of Markdown (headings, paragraphs, hr, fenced code,
// **bold**, `code`) into an existing DOM node. Built with createElement +
// textContent so all user content is auto-escaped — never use innerHTML with
// raw conversation bytes.
function appendMarkdownToElement(parent: HTMLElement, md: string) {
  const doc = parent.ownerDocument
  if (!doc) return
  const lines = md.split('\n')
  let i = 0
  let inCode = false
  let codeBuffer: string[] = []

  const flushCode = () => {
    if (codeBuffer.length === 0) return
    const pre = doc.createElement('pre')
    const code = doc.createElement('code')
    code.textContent = codeBuffer.join('\n')
    pre.appendChild(code)
    parent.appendChild(pre)
    codeBuffer = []
  }

  while (i < lines.length) {
    const line = lines[i]
    if (line.startsWith('```')) {
      if (inCode) {
        flushCode()
        inCode = false
      } else {
        inCode = true
      }
      i++
      continue
    }
    if (inCode) {
      codeBuffer.push(line)
      i++
      continue
    }
    if (line.startsWith('# ')) {
      const h = doc.createElement('h1')
      h.textContent = line.slice(2)
      parent.appendChild(h)
    } else if (line.startsWith('### ')) {
      const h = doc.createElement('h3')
      h.textContent = line.slice(4)
      parent.appendChild(h)
    } else if (/^(\s*)(-{3,}|\*{3,}|_{3,})\s*$/.test(line)) {
      parent.appendChild(doc.createElement('hr'))
    } else if (line.trim() === '') {
      // paragraph break — skip
    } else {
      const p = doc.createElement('p')
      p.textContent = line
      parent.appendChild(p)
    }
    i++
  }
  if (inCode) flushCode()
}

export default function Chat() {
  const {
    messages, streamingText, thinkingText, isQuerying, activeToolCalls, usage,
    sendMessage, cancelQuery,
  } = useChat()
  const {
    sessions, currentSessionId,
    createSession, switchSession, deleteSession, renameSession,
  } = useSessions()
  const { error, config, status } = useCatalog()
  const intl = useIntl()
  const navigate = useNavigate()
  const location = useLocation()
  const t = (id: string) => intl.formatMessage({ id })

  const [input, setInput] = useState('')

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
  const [sessionSearch, setSessionSearch] = useState('')
  const [backendSessionHits, setBackendSessionHits] = useState<SessionInfo[] | null>(null)
  const [editingSessionId, setEditingSessionId] = useState<string | null>(null)
  const [editTitle, setEditTitle] = useState('')
  const [diffPath, setDiffPath] = useState<string | null>(null)
  const [diffPaths, setDiffPaths] = useState<string[] | null>(null)
  const [attachedFiles, setAttachedFiles] = useState<string[]>([])
  const [pinnedIds, setPinnedIds] = useState<Set<string>>(new Set())
  const [sessionPage, setSessionPage] = useState(1)
  const [deleteTarget, setDeleteTarget] = useState<string | null>(null)
  const [quickFixOpen, setQuickFixOpen] = useState(false)
  const [editorOpen, setEditorOpen] = useState(false)

  const quickFixRef = useRef<HTMLDivElement>(null)
  useModalFocus(quickFixOpen, quickFixRef)
  const editorRef = useRef<HTMLDivElement>(null)
  useModalFocus(editorOpen, editorRef)
  const [contextPanelOpen, setContextPanelOpen] = useState(false)
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

  // Debounced backend full-text search. Backend matches title first, then
  // message content. Short queries fall back to a client-side title filter
  // (cheaper, instant feedback, no IPC round-trip).
  useEffect(() => {
    const q = sessionSearch.trim()
    if (q.length < 3) {
      setBackendSessionHits(null)
      return
    }
    let cancelled = false
    const handle = setTimeout(() => {
      api.searchSessions(q)
        .then(hits => { if (!cancelled) setBackendSessionHits(hits) })
        .catch(e => {
          console.warn('searchSessions failed, falling back to client filter:', e)
          if (!cancelled) setBackendSessionHits(null)
        })
    }, 250)
    return () => { cancelled = true; clearTimeout(handle) }
  }, [sessionSearch])

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

  const filteredSessions = useMemo(() => {
    const q = sessionSearch.trim()
    if (!q) return sessions
    if (backendSessionHits === null) {
      const ql = q.toLowerCase()
      return sessions.filter(s => s.title.toLowerCase().includes(ql))
    }
    const byId = new Map(sessions.map(s => [s.id, s]))
    return backendSessionHits.map(h => byId.get(h.id) ?? h)
  }, [sessions, sessionSearch, backendSessionHits])

  const sortedSessions = [...filteredSessions].sort((a, b) => {
    const aPin = pinnedIds.has(a.id) ? 1 : 0
    const bPin = pinnedIds.has(b.id) ? 1 : 0
    return bPin - aPin
  })

  const SESSIONS_PER_PAGE = 10
  const sessionTotalPages = Math.ceil(sortedSessions.length / SESSIONS_PER_PAGE)
  const pagedSessions = sortedSessions.slice((sessionPage - 1) * SESSIONS_PER_PAGE, sessionPage * SESSIONS_PER_PAGE)

  const togglePin = (id: string) => {
    setPinnedIds(prev => {
      const next = new Set(prev)
      if (next.has(id)) next.delete(id); else next.add(id)
      return next
    })
  }

  const handleExport = async (id: string) => {
    try {
      const md = await api.exportSession(id, 'markdown')
      const session = sessions.find(s => s.id === id)
      const defaultName = `${(session?.title || t('chat.export.defaultName')).replace(/[^a-z0-9-_]+/gi, '_').slice(0, 60)}.md`
      const target = await saveDialog({ defaultPath: defaultName, filters: [{ name: t('chat.export.markdown'), extensions: ['md'] }] })
      if (!target) return // user cancelled
      await api.saveTextFile(target, md)
      toast.success(t('chat.toast.exported'), { description: target })
    } catch (e) {
      console.warn('Export failed:', e)
      toast.error(t('chat.toast.exportFailed'), { description: String(e) })
    }
  }

  // Open a print-friendly window with the rendered conversation. The system
  // print dialog exposes "Save as PDF" on every desktop OS, which gives us
  // PDF export without dragging in a PDF library. DOM is built via
  // createElement + textContent so user content is auto-escaped — no string
  // interpolation into HTML.
  const handlePrint = async (id: string) => {
    try {
      const md = await api.exportSession(id, 'markdown')
      const session = sessions.find(s => s.id === id)
      const title = session?.title || t('chat.export.printTitle')
      const printWindow = window.open('', '_blank', 'width=900,height=700')
      if (!printWindow) {
        toast.error(t('chat.toast.popupBlocked'), { description: t('chat.toast.popupBlocked.desc') })
        return
      }
      const doc = printWindow.document
      doc.title = title
      const style = doc.createElement('style')
      style.textContent = buildPrintStyles({ variant: 'chat' })
      doc.head.appendChild(style)
      const h1 = doc.createElement('h1')
      h1.textContent = title
      doc.body.appendChild(h1)
      appendMarkdownToElement(doc.body, md)
      printWindow.focus()
      // Give the new window a tick to lay out before opening the print dialog.
      setTimeout(() => printWindow.print(), 250)
    } catch (e) {
      console.warn('Print failed:', e)
      toast.error(t('chat.toast.printFailed'), { description: String(e) })
    }
  }

  const formatTime = (ts: number) => {
    const d = new Date(ts)
    const now = new Date()
    if (d.toDateString() === now.toDateString()) return t('chat.time.today')
    const yesterday = new Date(now)
    yesterday.setDate(yesterday.getDate() - 1)
    if (d.toDateString() === yesterday.toDateString()) return t('chat.time.yesterday')
    return d.toLocaleDateString()
  }

  const untitled = t('chat.session.untitled')

  const currentSession = sessions.find(s => s.id === currentSessionId)
  const sessionWorkingDir = currentSession?.working_dir ?? config?.working_dir ?? ''

  const showApiKeyBanner =
    !bannerDismissed &&
    !!config &&
    !config.api_key &&
    config.provider !== 'ollama'

  const handleChangeWorkingDir = async () => {
    if (!currentSessionId) {
      toast.error(t('chat.header.workingDir.changeFailed'), { description: t('chat.header.workingDir.noSession') })
      return
    }
    try {
      const selected = await openDialog({ directory: true, multiple: false })
      if (!selected || Array.isArray(selected)) return
      await api.setSessionWorkingDir(currentSessionId, selected as string)
      toast.success(t('chat.header.workingDir.changed'), { description: selected as string })
    } catch (err) {
      toast.error(t('chat.header.workingDir.changeFailed'), { description: String(err) })
    }
  }
  changeWorkingDirRef.current = handleChangeWorkingDir

  const formatDirBreadcrumb = (full: string) => {
    const parts = full.replace(/\\/g, '/').split('/').filter(Boolean)
    if (parts.length <= 2) return full
    return '…/' + parts.slice(-2).join('/')
  }

  return (
    <ArtifactProvider>
    <div className="flex-1 flex w-full h-full relative">
      {/* Left Sidebar - Session History */}
      <aside className="hidden md:flex w-[220px] border-r border-outline-variant/10 flex-col glass-panel shrink-0 bg-surface-container-lowest/40">
        <div className="p-md border-b border-outline-variant/10">
          <Button
            className="w-full py-2 bg-primary text-on-primary rounded-lg font-bold flex items-center justify-center gap-2 hover:shadow-md active:scale-95 transition-all"
            onClick={createSession}
          >
            <span className="material-symbols-outlined text-[18px]">add</span>
            {t('chat.newChat')}
          </Button>
          <div className="relative mt-sm">
            <span className="material-symbols-outlined absolute left-sm top-1/2 -translate-y-1/2 text-on-surface-variant text-[18px]">search</span>
            <Input
              className="w-full pl-xl pr-md py-xs bg-surface-container border-none rounded-lg text-body-sm focus:ring-1 focus:ring-primary/30"
              placeholder={t('chat.searchSessions.placeholder')}
              type="text"
              value={sessionSearch}
              onChange={e => setSessionSearch(e.target.value)}
            />
          </div>
        </div>
        <ScrollArea className="flex-1 p-sm space-y-xs">
          {filteredSessions.length === 0 && (
            <div className="text-center py-lg opacity-70">
              <span className="material-symbols-outlined text-on-surface-variant text-[32px]">chat_bubble_outline</span>
              <p className="text-body-sm text-on-surface-variant mt-xs">{t('chat.empty.sessions')}</p>
            </div>
          )}
          {pagedSessions.map(session => (
            <div
              key={session.id}
              role="button"
              tabIndex={0}
              aria-label={intl.formatMessage({ id: 'chat.session.aria' }, { title: session.title || untitled })}
              className={`p-sm rounded-lg cursor-pointer group border-l-2 ${
                session.id === currentSessionId
                  ? 'bg-surface-container-high/60 border-primary'
                  : 'border-transparent hover:bg-surface-container-high/40'
              }`}
              onClick={() => switchSession(session.id)}
              onKeyDown={e => { if (e.key === 'Enter') switchSession(session.id); if (e.key === 'Delete') setDeleteTarget(session.id) }}
              onContextMenu={e => {
                e.preventDefault()
                setDeleteTarget(session.id)
              }}
              onDoubleClick={() => {
                setEditingSessionId(session.id)
                setEditTitle(session.title)
              }}
            >
              {editingSessionId === session.id ? (
                <Input
                  className="w-full text-sm py-0 px-xs"
                  value={editTitle}
                  onChange={e => setEditTitle(e.target.value)}
                  onBlur={() => {
                    renameSession(session.id, editTitle)
                    setEditingSessionId(null)
                  }}
                  onKeyDown={e => {
                    if (e.key === 'Enter') {
                      renameSession(session.id, editTitle)
                      setEditingSessionId(null)
                    }
                  }}
                  autoFocus
                />
              ) : (
                <>
                  <div className="flex items-center justify-between">
                    <p className={`font-label-md truncate flex-1 ${session.id === currentSessionId ? 'text-primary font-bold' : 'text-on-surface group-hover:text-primary transition-colors'}`}>
                      {pinnedIds.has(session.id) && <span className="material-symbols-outlined text-[14px] text-primary mr-xs align-text-bottom">push_pin</span>}
                      <HighlightText text={session.title || untitled} query={sessionSearch} />
                    </p>
                    <div className="flex items-center gap-xs opacity-0 group-hover:opacity-100 transition-opacity shrink-0">
                      <button className="p-xs rounded hover:bg-surface-container text-on-surface-variant hover:text-primary focus-visible:ring-2 focus-visible:ring-primary/30 focus-visible:outline-none" onClick={e => { e.stopPropagation(); togglePin(session.id) }} title={pinnedIds.has(session.id) ? t('chat.session.unpin') : t('chat.session.pin')} aria-pressed={pinnedIds.has(session.id)}>
                        <span className="material-symbols-outlined text-[14px]">{pinnedIds.has(session.id) ? 'push_pin' : 'keep'}</span>
                      </button>
                      <button className="p-xs rounded hover:bg-surface-container text-on-surface-variant hover:text-primary focus-visible:ring-2 focus-visible:ring-primary/30 focus-visible:outline-none" onClick={e => { e.stopPropagation(); handleExport(session.id) }} title={t('chat.session.export')} aria-label={intl.formatMessage({ id: 'chat.session.export.aria' }, { title: session.title || untitled })}>
                        <span className="material-symbols-outlined text-[14px]">download</span>
                      </button>
                      <button className="p-xs rounded hover:bg-surface-container text-on-surface-variant hover:text-primary focus-visible:ring-2 focus-visible:ring-primary/30 focus-visible:outline-none" onClick={e => { e.stopPropagation(); handlePrint(session.id) }} title={t('chat.session.print')} aria-label={intl.formatMessage({ id: 'chat.session.print.aria' }, { title: session.title || untitled })}>
                        <span className="material-symbols-outlined text-[14px]">print</span>
                      </button>
                    </div>
                  </div>
                  <p className="text-body-sm text-on-surface-variant opacity-70 truncate">
                    {intl.formatMessage({ id: 'chat.session.meta' }, { count: session.message_count, time: formatTime(session.created_at) })}
                  </p>
                  {session.working_dir && (
                    <p className="text-label-xs text-outline font-mono truncate mt-[2px] flex items-center gap-[4px]" title={session.working_dir}>
                      <span className="material-symbols-outlined icon-xs opacity-70">folder</span>
                      <span className="truncate">{formatDirBreadcrumb(session.working_dir)}</span>
                    </p>
                  )}
                </>
              )}
            </div>
          ))}
        </ScrollArea>
        <Pagination page={sessionPage} totalPages={sessionTotalPages} onPageChange={setSessionPage} />
      </aside>

      {/* Main Chat Canvas */}
      <section className="flex-1 flex flex-col relative bg-surface-container-lowest/40 overflow-hidden">
        {/* Header strip — session title + working directory breadcrumb */}
        <header
          role="banner"
          aria-label={t('chat.header.aria')}
          className="relative shrink-0 flex items-center gap-md px-lg py-sm bg-surface/60 backdrop-blur-sm border-b border-outline-variant/15"
        >
          <div className="flex items-center gap-sm min-w-0 flex-1">
            <span className="material-symbols-outlined text-primary text-[20px] shrink-0">forum</span>
            <div className="min-w-0 flex-1">
              <h2 className="font-headline-sm font-bold text-on-surface truncate leading-tight">
                {currentSession?.title || untitled || t('chat.empty.start')}
              </h2>
              {sessionWorkingDir && (
                <button
                  type="button"
                  onClick={handleChangeWorkingDir}
                  className="mt-[2px] flex items-center gap-xs text-label-sm text-on-surface-variant hover:text-primary transition-colors max-w-full"
                  title={sessionWorkingDir}
                >
                  <span className="material-symbols-outlined text-[14px] opacity-70 shrink-0">folder</span>
                  <span className="truncate font-mono">{formatDirBreadcrumb(sessionWorkingDir)}</span>
                </button>
              )}
            </div>
          </div>
          <button
            type="button"
            onClick={() => setContextPanelOpen(v => !v)}
            className="p-xs rounded-lg text-on-surface-variant hover:text-primary hover:bg-surface-container focus-visible:ring-2 focus-visible:ring-primary/30 focus-visible:outline-none shrink-0"
            title={t('chat.header.contextPanel.toggle')}
            aria-label={t('chat.header.contextPanel.toggle')}
            aria-expanded={contextPanelOpen}
            aria-pressed={contextPanelOpen}
          >
            <span className="material-symbols-outlined icon-md">{contextPanelOpen ? 'right_panel_close' : 'right_panel_open'}</span>
          </button>
        </header>

        {showApiKeyBanner && (
          <Banner
            tone="info"
            className="shannon-apikey-banner"
            onDismiss={() => setBannerDismissed(true)}
            dismissLabel={t('chat.banner.apiKeyMissing.dismiss')}
          >
            <span className="material-symbols-outlined text-secondary icon-md shrink-0 mt-[2px]">key_alert</span>
            <div className="flex-1 min-w-0">
              <p className="font-label-md text-on-surface">{t('chat.banner.apiKeyMissing.title')}</p>
              <p className="font-body-sm text-on-surface-variant mt-xs">{t('chat.banner.apiKeyMissing.body')}</p>
            </div>
            <button
              type="button"
              onClick={() => navigate('/settings/models')}
              className="shannon-apikey-banner-cta shrink-0 px-md py-xs bg-primary text-on-primary rounded-lg font-label-md cursor-pointer hover:bg-primary/90 focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-primary"
            >
              {t('chat.banner.apiKeyMissing.cta')}
            </button>
          </Banner>
        )}

        {/* Message Area — virtualized list for chat history */}
        <div ref={scrollParentRef} className="flex-1 overflow-y-auto px-xl pt-lg pb-32">
          {messages.length === 0 && !streamingText && (
            <WelcomeState onSelectPrompt={setInput} />
          )}

          {messages.length > 0 && shouldVirtualize && (
            <div
              style={{ height: `${virtualizer.getTotalSize()}px`, position: 'relative' }}
              aria-label={t('chat.history.aria')}
            >
              {virtualizer.getVirtualItems().map(vItem => {
                const msg = messages[vItem.index]
                return (
                  <div
                    key={`${msg.timestamp}-${vItem.index}`}
                    data-index={vItem.index}
                    ref={virtualizer.measureElement}
                    className="pb-lg"
                    style={{ position: 'absolute', top: 0, left: 0, width: '100%', transform: `translateY(${vItem.start}px)` }}
                  >
                    <MessageBubble message={msg} messageIndex={vItem.index} onViewDiff={setDiffPath} onViewDiffMulti={setDiffPaths} />
                  </div>
                )
              })}
            </div>
          )}

          {messages.length > 0 && !shouldVirtualize && (
            <div aria-label={t('chat.history.aria')}>
              {messages.map((msg, i) => (
                <div key={`${msg.timestamp}-${i}`} className="pb-lg">
                  <MessageBubble message={msg} messageIndex={i} onViewDiff={setDiffPath} onViewDiffMulti={setDiffPaths} />
                </div>
              ))}
            </div>
          )}

          {/* Streaming response */}
          {(streamingText || thinkingText || activeToolCalls.length > 0) && (
            <StreamingResponse
              streamingText={streamingText}
              thinkingText={thinkingText}
              activeToolCalls={activeToolCalls}
              onViewDiff={setDiffPath}
            />
          )}

          {error && (
            <div className="mx-auto max-w-md p-md bg-error/10 border border-error/20 rounded-xl text-center">
              <p className="text-body-sm text-error">{error}</p>
              <Button variant="ghost" className="mt-sm text-error hover:bg-error/10 text-label-md cursor-pointer" onClick={() => { if (input.trim()) handleSend() }}>{t('chat.error.retry')}</Button>
            </div>
          )}

          <div ref={messagesEndRef} />
        </div>

        {/* Input Bar */}
        <div className="absolute bottom-6 md:bottom-12 w-full px-lg md:px-xl py-lg transition-colors">
          <div className="max-w-4xl mx-auto">
            <div className="bg-surface-container-lowest border border-outline-variant/30 rounded-2xl shadow-sm">
              <ChatInput
                value={input}
                onChange={setInput}
                onSend={handleSend}
                attachedFiles={attachedFiles}
                onAttach={handleAttach}
                onDetachAll={handleDetachAll}
                disabled={isQuerying}
                isQuerying={isQuerying}
                onCancelQuery={cancelQuery}
                currentSessionId={currentSessionId}
                sessionWorkingDir={sessionWorkingDir}
                onOpenQuickFix={() => setQuickFixOpen(true)}
                onOpenEditor={() => setEditorOpen(true)}
              />
            </div>
            <div className="mt-xs flex items-center justify-between gap-md px-sm text-label-sm text-on-surface-variant">
              <button
                type="button"
                onClick={handleChangeWorkingDir}
                disabled={!currentSessionId}
                className="flex items-center gap-xs min-w-0 hover:text-primary transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
                title={sessionWorkingDir || t('chat.input.footer.workingDir.unset')}
                aria-label={t('chat.input.footer.workingDir.aria')}
              >
                <span className="material-symbols-outlined text-[14px] shrink-0">folder</span>
                <span className="truncate font-mono">
                  {sessionWorkingDir ? formatDirBreadcrumb(sessionWorkingDir) : t('chat.input.footer.workingDir.unset')}
                </span>
              </button>
              {status && (
                <span
                  className="flex items-center gap-xs shrink-0 font-mono"
                  title={`${status.provider} · ${status.model}`}
                  aria-label={t('chat.input.footer.model.aria')}
                >
                  <span className="w-1.5 h-1.5 rounded-full bg-tertiary animate-pulse"></span>
                  <span className="truncate max-w-[200px]">{status.provider}/{status.model}</span>
                </span>
              )}
            </div>
          </div>
        </div>
      </section>

      {/* Delete Confirmation Modal */}
      {deleteTarget && (
        <div className="fixed inset-0 z-[80] bg-black/30 backdrop-blur-sm flex items-center justify-center" onClick={() => setDeleteTarget(null)} onKeyDown={e => { if (e.key === 'Escape') setDeleteTarget(null) }}>
          <div className="bg-surface-container-lowest rounded-2xl p-xl shadow-xl border border-outline-variant/30 max-w-sm w-full mx-md" onClick={e => e.stopPropagation()}>
            <div className="flex items-center gap-sm mb-md">
              <span className="material-symbols-outlined text-error text-[24px]">delete</span>
              <h3 className="font-headline-md text-on-surface">{t('chat.delete.title')}</h3>
            </div>
            <p className="text-body-md text-on-surface-variant mb-lg">{t('chat.delete.confirm')}</p>
            <div className="flex justify-end gap-sm">
              <Button className="px-lg py-sm rounded-xl text-on-surface-variant hover:bg-surface-container" onClick={() => setDeleteTarget(null)}>{t('chat.delete.cancel')}</Button>
              <Button className="px-lg py-sm rounded-xl bg-error text-on-error hover:bg-error/90" onClick={() => { deleteSession(deleteTarget); setDeleteTarget(null) }}>{t('chat.delete.confirmButton')}</Button>
            </div>
          </div>
        </div>
      )}

      {/* Inline QuickFix panel — opened from the chat input toolbar. */}
      {quickFixOpen && (
        <div
          ref={quickFixRef}
          role="dialog"
          aria-modal="true"
          aria-label={t('nav.quickFix')}
          className="fixed inset-0 z-[85] bg-black/40 backdrop-blur-sm flex items-center justify-center p-lg"
          onClick={() => setQuickFixOpen(false)}
          onKeyDown={e => { if (e.key === 'Escape') setQuickFixOpen(false) }}
        >
          <div
            className="bg-surface-container-lowest rounded-2xl shadow-2xl border border-outline-variant/30 w-full max-w-3xl max-h-[85vh] overflow-y-auto"
            onClick={e => e.stopPropagation()}
          >
            <div className="sticky top-0 z-10 flex items-center justify-between px-lg py-md bg-surface-container-lowest/95 backdrop-blur-md border-b border-outline-variant/20">
              <h3 className="font-headline-md text-on-surface">{t('nav.quickFix')}</h3>
              <Button variant="ghost" aria-label={t('chat.delete.cancel')} onClick={() => setQuickFixOpen(false)}>
                <span className="material-symbols-outlined">close</span>
              </Button>
            </div>
            <div className="p-lg">
              <Suspense fallback={<div className="flex items-center justify-center py-xl"><span className="material-symbols-outlined animate-spin text-primary">progress_activity</span></div>}>
                <QuickFixPanel />
              </Suspense>
            </div>
          </div>
        </div>
      )}

      {/* Inline Editor panel — opened from the chat input toolbar. */}
      {editorOpen && (
        <div
          ref={editorRef}
          role="dialog"
          aria-modal="true"
          aria-label={t('nav.editor')}
          className="fixed inset-0 z-[85] bg-black/40 backdrop-blur-sm flex items-center justify-center p-md"
          onClick={() => setEditorOpen(false)}
          onKeyDown={e => { if (e.key === 'Escape') setEditorOpen(false) }}
        >
          <div
            className="bg-surface-container-lowest rounded-2xl shadow-2xl border border-outline-variant/30 w-full max-w-5xl h-[90vh] flex flex-col"
            onClick={e => e.stopPropagation()}
          >
            <div className="flex items-center justify-between px-lg py-md bg-surface-container-lowest/95 backdrop-blur-md border-b border-outline-variant/20">
              <h3 className="font-headline-md text-on-surface">{t('nav.editor')}</h3>
              <Button variant="ghost" aria-label={t('chat.delete.cancel')} onClick={() => setEditorOpen(false)}>
                <span className="material-symbols-outlined">close</span>
              </Button>
            </div>
            <div className="flex-1 overflow-hidden">
              <Suspense fallback={<div className="flex items-center justify-center py-xl"><span className="material-symbols-outlined animate-spin text-primary">progress_activity</span></div>}>
                <EditorPanel />
              </Suspense>
            </div>
          </div>
        </div>
      )}

      {/* Right Sidebar - Context (collapsible) */}
      <aside
        aria-label={t('chat.context.aria')}
        className="glass-panel shrink-0 overflow-y-auto p-lg border-l border-outline-variant/10 bg-surface-container-lowest/50 transition-all duration-300 ease-in-out"
        style={{
          width: contextPanelOpen ? 300 : 0,
          padding: contextPanelOpen ? undefined : 0,
          borderWidth: contextPanelOpen ? undefined : 0,
          opacity: contextPanelOpen ? 1 : 0,
        }}
      >
        <div className="space-y-xl">
          {/* Token Usage */}
          {usage && (
            <section>
              <h3 className="font-label-md text-on-surface uppercase tracking-wider opacity-60 mb-md">{t('chat.context.usage')}</h3>
              <div className="p-md bg-surface-container rounded-xl border border-outline-variant/10 space-y-sm">
                <div className="flex justify-between text-body-sm">
                  <span className="text-on-surface-variant">{t('chat.context.inputTokens')}</span>
                  <span className="font-bold text-on-surface">{usage.input_tokens.toLocaleString()}</span>
                </div>
                <div className="flex justify-between text-body-sm">
                  <span className="text-on-surface-variant">{t('chat.context.outputTokens')}</span>
                  <span className="font-bold text-on-surface">{usage.output_tokens.toLocaleString()}</span>
                </div>
                <div className="flex justify-between text-body-sm">
                  <span className="text-on-surface-variant">{t('chat.context.cost')}</span>
                  <span className="font-bold text-primary">${usage.cost_usd.toFixed(4)}</span>
                </div>
                {(() => {
                  const total = usage.input_tokens + usage.output_tokens
                  const max = usage.max_tokens
                  if (!max) return null
                  const pct = Math.min(100, (total / max) * 100)
                  const barColor = pct > 80 ? 'bg-error' : pct > 50 ? 'bg-secondary' : 'bg-primary'
                  return (
                    <div className="pt-sm border-t border-outline-variant/10">
                      <div className="flex justify-between text-label-sm text-on-surface-variant mb-xs">
                        <span>{t('chat.context.window')}</span>
                        <span className="font-bold">{pct.toFixed(0)}%</span>
                      </div>
                      <div className="w-full h-1.5 bg-surface-container-high rounded-full overflow-hidden">
                        <div className={`h-full rounded-full transition-all duration-500 ${barColor}`} style={{ width: `${pct}%` }} />
                      </div>
                      <p className="text-label-sm text-on-surface-variant mt-xs">{total.toLocaleString()} / {max.toLocaleString()}</p>
                    </div>
                  )
                })()}
              </div>
            </section>
          )}

          {/* Active Tool Calls */}
          {activeToolCalls.length > 0 && (
            <section>
              <h3 className="font-label-md text-on-surface uppercase tracking-wider opacity-60 mb-md">
                {t('chat.context.activeTools')}
                <span className="ml-xs px-xs py-[2px] bg-primary/10 text-primary text-[10px] font-bold rounded">{activeToolCalls.length}</span>
              </h3>
              <div className="space-y-sm">
                {activeToolCalls.map(tc => (
                  <div key={tc.tool_use_id} className="p-sm bg-surface-container rounded-xl flex items-center gap-sm border border-outline-variant/10">
                    <span className={`w-2 h-2 rounded-full shrink-0 ${tc.status === 'running' ? 'bg-secondary animate-pulse' : tc.status === 'error' ? 'bg-error' : 'bg-tertiary'}`}></span>
                    <p className="text-label-md truncate">{tc.tool_name}</p>
                  </div>
                ))}
              </div>
            </section>
          )}
        </div>
      </aside>
      <DiffDialog open={diffPath !== null} filePath={diffPath} onClose={() => setDiffPath(null)} />
      <DiffDialogMulti open={diffPaths !== null} filePaths={diffPaths ?? []} onClose={() => setDiffPaths(null)} />
      <ArtifactPanel />
    </div>
    </ArtifactProvider>
  )
}

const HighlightText = memo(function HighlightText({ text, query }: { text: string; query: string }) {
  if (!query) return <>{text}</>
  const idx = text.toLowerCase().indexOf(query.toLowerCase())
  if (idx === -1) return <>{text}</>
  return (
    <>
      {text.slice(0, idx)}
      <mark className="bg-primary/20 text-inherit rounded-sm px-[1px]">{text.slice(idx, idx + query.length)}</mark>
      {text.slice(idx + query.length)}
    </>
  )
});
