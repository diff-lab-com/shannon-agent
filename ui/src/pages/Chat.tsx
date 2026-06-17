import { useState, useRef, useEffect, useMemo, memo, lazy, Suspense } from 'react'
import { useIntl } from 'react-intl'
import ReactMarkdown from 'react-markdown'
import { toast } from 'sonner'
import remarkGfm from 'remark-gfm'
import rehypeHighlight from 'rehype-highlight'
import { open as openDialog, save as saveDialog } from '@tauri-apps/plugin-dialog'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { ScrollArea } from '@/components/ui/scroll-area'
import { Pagination } from '@/components/ui/pagination'
import WelcomeState from '@/components/WelcomeState'
import DiffDialog from '@/components/diff/DiffDialog'
import { useApp } from '@/context/AppContext'
import * as api from '@/lib/tauri-api'
import type { ChatMessage, ToolCall, FileContext, SessionInfo } from '@/types'

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
    sessions, currentSessionId, error, config, status,
    sendMessage, cancelQuery, createSession, switchSession, deleteSession, renameSession,
  } = useApp()
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })

  const [input, setInput] = useState('')
  const [sessionSearch, setSessionSearch] = useState('')
  const [backendSessionHits, setBackendSessionHits] = useState<SessionInfo[] | null>(null)
  const [editingSessionId, setEditingSessionId] = useState<string | null>(null)
  const [editTitle, setEditTitle] = useState('')
  const [diffPath, setDiffPath] = useState<string | null>(null)
  const [fileContext, setFileContext] = useState<FileContext[]>([])
  const [attachedFiles, setAttachedFiles] = useState<string[]>([])
  const [pinnedIds, setPinnedIds] = useState<Set<string>>(new Set())
  const [sessionPage, setSessionPage] = useState(1)
  const [deleteTarget, setDeleteTarget] = useState<string | null>(null)
  const [quickFixOpen, setQuickFixOpen] = useState(false)
  const [editorOpen, setEditorOpen] = useState(false)
  const messagesEndRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    messagesEndRef.current?.scrollIntoView({ behavior: 'smooth' })
  }, [messages, streamingText])

  useEffect(() => {
    api.getFileContext().then(setFileContext).catch(e => console.warn('Failed to load file context:', e))
  }, [messages])

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
  const handleAttach = async () => {
    try {
      const selected = await openDialog({
        multiple: true,
        filters: [
          { name: t('chat.export.documents'), extensions: ['pdf', 'png', 'jpg', 'jpeg', 'gif', 'webp'] },
        ],
      })
      if (!selected) return
      const paths = (Array.isArray(selected) ? selected : [selected]) as string[]
      if (paths.length > 0) setAttachedFiles(prev => [...prev, ...paths])
    } catch (err) {
      toast.error(t('chat.toast.attachFailed'), { description: String(err) })
    }
  }

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault()
      handleSend()
    }
    if (e.key === 'Escape' && isQuerying) {
      cancelQuery()
    }
    if (e.key === 'ArrowUp' && e.altKey && input === '' && messages.length > 0) {
      e.preventDefault()
      const lastUserMsg = [...messages].reverse().find(m => m.role === 'user')
      if (lastUserMsg) setInput(lastUserMsg.content)
    }
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
      style.textContent = `
        body { font: 14px/1.6 -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif; padding: 32px; color: #111; max-width: 760px; margin: 0 auto; }
        h1 { font-size: 22px; margin-bottom: 4px; }
        h3 { font-size: 14px; margin-top: 24px; color: #555; text-transform: uppercase; letter-spacing: 0.04em; }
        hr { border: 0; border-top: 1px solid #ddd; margin: 16px 0; }
        pre { background: #f5f5f5; padding: 12px; border-radius: 6px; overflow-x: auto; }
        code { font-family: ui-monospace, 'SF Mono', Menlo, monospace; font-size: 13px; }
        p { white-space: pre-wrap; }
        strong { font-weight: 600; }
      `
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

  const handleChangeWorkingDir = async () => {
    if (!currentSessionId) {
      toast.error(t('chat.header.workingDir.changeFailed'), { description: t('chat.empty.start') })
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

  const formatDirBreadcrumb = (full: string) => {
    const parts = full.replace(/\\/g, '/').split('/').filter(Boolean)
    if (parts.length <= 2) return full
    return '…/' + parts.slice(-2).join('/')
  }

  return (
    <div className="flex-1 flex w-full h-full relative">
      {/* Left Sidebar - Session History */}
      <aside className="hidden md:flex w-[240px] border-r border-outline-variant/10 flex-col glass-panel shrink-0 bg-surface-container-lowest/40">
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
              className={`p-sm rounded-lg cursor-pointer group ${
                session.id === currentSessionId
                  ? 'bg-primary-fixed/40 border-l-4 border-primary shadow-sm'
                  : 'hover:bg-surface-container-high/50'
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
                      <span className="material-symbols-outlined text-[12px] opacity-70">folder</span>
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
        {/* Ambient backdrop — subtle radial accents for depth without distraction */}
        <div aria-hidden="true" className="pointer-events-none absolute inset-0 opacity-60">
          <div className="absolute -top-32 -right-32 w-96 h-96 rounded-full bg-primary/5 blur-3xl"></div>
          <div className="absolute top-1/3 -left-32 w-80 h-80 rounded-full bg-tertiary/5 blur-3xl"></div>
        </div>

        {/* Header strip — session title + working directory breadcrumb */}
        <header
          role="banner"
          aria-label={t('chat.header.aria')}
          className="relative shrink-0 flex items-center gap-md px-lg py-sm border-b border-outline-variant/15 bg-gradient-to-r from-surface-container-lowest/80 via-surface-container-low/40 to-surface-container-lowest/80 backdrop-blur-sm"
        >
          <div className="flex items-center gap-sm min-w-0 flex-1">
            <span className="material-symbols-outlined text-primary text-[20px] shrink-0">forum</span>
            <div className="min-w-0 flex-1">
              <p className="text-label-xs uppercase tracking-wider text-on-surface-variant opacity-70 leading-none">
                {t('chat.header.sessionLabel')}
              </p>
              <h2 className="font-label-lg font-bold text-on-surface truncate leading-tight mt-[2px]">
                {currentSession?.title || untitled || t('chat.empty.start')}
              </h2>
            </div>
          </div>
          <button
            type="button"
            onClick={handleChangeWorkingDir}
            aria-label={t('chat.header.workingDir.aria')}
            title={sessionWorkingDir
              ? intl.formatMessage({ id: 'chat.header.workingDir.tooltip' }, { path: sessionWorkingDir })
              : t('chat.header.workingDir.unset')}
            className={`group flex items-center gap-xs px-sm py-xs rounded-full text-label-sm border transition-all shrink-0 ${
              sessionWorkingDir
                ? 'border-primary/30 bg-primary/5 text-on-surface hover:bg-primary/10 hover:border-primary/50'
                : 'border-outline-variant/30 bg-surface-container-lowest/60 text-on-surface-variant hover:bg-surface-container-low hover:border-outline-variant hover:text-primary'
            }`}
          >
            <span className="material-symbols-outlined text-[16px]">folder_open</span>
            <span className="max-w-[180px] truncate font-mono">
              {sessionWorkingDir ? formatDirBreadcrumb(sessionWorkingDir) : t('chat.header.workingDir.unset')}
            </span>
            <span className="material-symbols-outlined text-[14px] opacity-50 group-hover:opacity-100 group-hover:text-primary transition-opacity">change_folder</span>
          </button>
          {status && (
            <div className="hidden md:flex items-center gap-xs px-sm py-xs rounded-full bg-surface-container-lowest/60 border border-outline-variant/20 shrink-0" title={`${status.provider} · ${status.model}`}>
              <span className="w-1.5 h-1.5 rounded-full bg-tertiary animate-pulse"></span>
              <span className="text-label-sm text-on-surface-variant font-mono truncate max-w-[160px]">{status.provider}/{status.model}</span>
            </div>
          )}
        </header>

        {/* Message Area */}
        <ScrollArea className="flex-1 px-xl pt-lg space-y-lg pb-32">
          {messages.length === 0 && !streamingText && (
            sessions.length === 0 ? (
              <WelcomeState onSelectPrompt={setInput} />
            ) : (
              <div className="flex items-center justify-center h-full opacity-40">
                <div className="text-center space-y-sm">
                  <span className="material-symbols-outlined text-[48px] text-primary">chat_bubble</span>
                  <p className="font-body-lg text-on-surface-variant">{t('chat.empty.start')}</p>
                </div>
              </div>
            )
          )}

          {messages.map((msg, i) => (
            <MessageBubble key={`${msg.timestamp}-${i}`} message={msg} onViewDiff={setDiffPath} />
          ))}

          {/* Streaming response */}
          {(streamingText || thinkingText || activeToolCalls.length > 0) && (
            <div className="flex gap-md max-w-[90%]" aria-live="polite" aria-label={t('chat.streaming.aria')}>
              <div className="h-10 w-10 rounded-full bg-primary-container flex items-center justify-center shrink-0 shadow-md">
                <span className="material-symbols-outlined text-on-primary-container">smart_toy</span>
              </div>
              <div className="space-y-md flex-1">
                {thinkingText && (
                  <div className="bg-surface-container-lowest p-md rounded-xl border border-outline-variant/10">
                    <div className="relative pl-6">
                      <div className="absolute left-0 top-1 h-4 w-4 rounded-full bg-primary/20 flex items-center justify-center">
                        <div className="h-1.5 w-1.5 rounded-full bg-primary animate-pulse"></div>
                      </div>
                      <span className="font-label-sm text-on-surface-variant block uppercase opacity-70">{t('chat.streaming.thinking')}</span>
                      <p className="text-body-sm whitespace-pre-wrap">{thinkingText}</p>
                    </div>
                  </div>
                )}
                {activeToolCalls.map(tc => (
                  <ToolCallDisplay key={tc.tool_use_id} toolCall={tc} onViewDiff={setDiffPath} />
                ))}
                {streamingText && (
                  <div className="bg-surface-container-lowest px-lg py-md rounded-2xl rounded-tl-none border border-outline-variant/20 shadow-sm">
                    <p className="font-body-md text-on-surface whitespace-pre-wrap">{streamingText}<span className="inline-block w-2 h-5 bg-primary/60 ml-xs animate-pulse align-text-bottom"></span></p>
                  </div>
                )}
              </div>
            </div>
          )}

          {error && (
            <div className="mx-auto max-w-md p-md bg-error/10 border border-error/20 rounded-xl text-center">
              <p className="text-body-sm text-error">{error}</p>
              <Button variant="ghost" className="mt-sm text-error hover:bg-error/10 text-label-md cursor-pointer" onClick={() => { if (input.trim()) handleSend() }}>{t('chat.error.retry')}</Button>
            </div>
          )}

          <div ref={messagesEndRef} />
        </ScrollArea>

        {/* Input Bar */}
        <div
          className="absolute bottom-6 md:bottom-12 w-full px-lg md:px-xl py-lg bg-gradient-to-t from-background via-background/90 to-transparent transition-colors"
        >
          <div className="max-w-4xl mx-auto relative group">
            <div className="absolute inset-0 bg-primary/10 blur-xl rounded-full opacity-50 group-focus-within:opacity-100 transition-opacity duration-500"></div>
            {attachedFiles.length > 0 && (
              <div className="flex flex-wrap gap-xs mb-sm relative">
                {attachedFiles.map((path, i) => (
                  <span key={i} className="inline-flex items-center gap-xs px-sm py-xs bg-primary/10 text-primary rounded-lg font-label-sm">
                    <span className="material-symbols-outlined text-[14px]">description</span>
                    {path.split('/').pop()}
                    <button className="hover:text-error cursor-pointer" onClick={() => setAttachedFiles(prev => prev.filter((_, j) => j !== i))}>
                      <span className="material-symbols-outlined text-[14px]">close</span>
                    </button>
                  </span>
                ))}
              </div>
            )}
            <div className="relative glass-card bg-surface-container-lowest/80 rounded-2xl border border-outline-variant/30 px-sm py-xs flex items-center shadow-lg group-focus-within:border-primary/50 group-focus-within:shadow-primary/10 transition-all duration-300">
              <Button variant="ghost" aria-label={t('chat.input.attach.aria')} className="p-md text-on-surface-variant hover:text-primary" onClick={handleAttach}>
                <span className="material-symbols-outlined text-[20px]" aria-hidden="true">attach_file</span>
              </Button>
              <Button variant="ghost" aria-label={t('nav.quickFix')} title={t('nav.quickFix')} className="p-md text-on-surface-variant hover:text-primary" onClick={() => setQuickFixOpen(true)}>
                <span className="material-symbols-outlined text-[20px]" aria-hidden="true">build</span>
              </Button>
              <Button variant="ghost" aria-label={t('nav.editor')} title={t('nav.editor')} className="p-md text-on-surface-variant hover:text-primary" onClick={() => setEditorOpen(true)}>
                <span className="material-symbols-outlined text-[20px]" aria-hidden="true">code</span>
              </Button>
              <span className="material-symbols-outlined p-md text-primary" aria-hidden="true">{isQuerying ? 'hourglass_empty' : 'auto_awesome'}</span>
              <textarea
                className="flex-1 bg-transparent border-none outline-none focus:ring-0 font-body-lg py-md px-sm placeholder:text-outline-variant/80 text-on-surface resize-none min-h-[24px] max-h-[200px]"
                placeholder={isQuerying ? t('chat.input.processing') : t('chat.input.placeholder')}
                value={input}
                onChange={e => setInput(e.target.value)}
                onKeyDown={handleKeyDown}
                rows={1}
                disabled={isQuerying}
              />
              <div className="flex items-center gap-2 px-sm">
                {isQuerying ? (
                  <Button aria-label={t('chat.input.stop.aria')} className="bg-error/80 text-on-error p-3 rounded-xl active:scale-95 transition-all" onClick={cancelQuery}>
                    <span className="material-symbols-outlined text-[20px]" aria-hidden="true">stop</span>
                  </Button>
                ) : (
                  <Button
                    aria-label={t('chat.input.send.aria')}
                    className="bg-primary text-on-primary p-3 rounded-xl active:scale-95 hover:shadow-md hover:shadow-primary/30 transition-all disabled:opacity-40 disabled:cursor-not-allowed"
                    onClick={handleSend}
                    disabled={!input.trim()}
                  >
                    <span className="material-symbols-outlined text-[20px]" aria-hidden="true">arrow_upward</span>
                  </Button>
                )}
              </div>
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

      {/* Right Sidebar - Context */}
      <aside aria-label={t('chat.context.aria')} className="w-[300px] border-l border-outline-variant/10 glass-panel shrink-0 p-lg overflow-y-auto bg-surface-container-lowest/50 hidden lg:block">
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
                  const max = (usage as any).max_tokens ?? 200000
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

          {/* File Context */}
          {fileContext.length > 0 && (
            <section>
              <h3 className="font-label-md text-on-surface uppercase tracking-wider opacity-60 mb-md">
                {t('chat.context.files')}
                <span className="ml-xs px-xs py-[2px] bg-secondary/10 text-secondary text-[10px] font-bold rounded">{fileContext.length}</span>
              </h3>
              <div className="space-y-sm">
                {fileContext.map(fc => (
                  <div key={fc.path} className="p-sm bg-surface-container rounded-xl border border-outline-variant/10">
                    <div className="flex items-center gap-sm mb-xs">
                      <span className="material-symbols-outlined text-[16px] text-primary" aria-hidden="true">description</span>
                      <p className="text-label-md text-on-surface truncate flex-1" title={fc.path}>{fc.name}</p>
                    </div>
                    <div className="flex items-center gap-md text-label-sm text-on-surface-variant">
                      <span>{fc.language}</span>
                      <span>{intl.formatMessage({ id: 'chat.context.lines' }, { count: fc.lines })}</span>
                    </div>
                  </div>
                ))}
              </div>
            </section>
          )}
        </div>
      </aside>
      <DiffDialog open={diffPath !== null} filePath={diffPath} onClose={() => setDiffPath(null)} />
    </div>
  )
}

const MessageBubble = memo(function MessageBubble({ message, isBranch, onViewDiff }: { message: ChatMessage; isBranch?: boolean; onViewDiff: (path: string) => void }) {
  const isUser = message.role === 'user'
  const [liked, setLiked] = useState(false)
  const { sendMessage } = useApp()
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })

  const handleCopy = () => {
    navigator.clipboard.writeText(message.content).catch(() => toast.error(t('chat.toast.copyFailed')))
  }

  const handleRegenerate = () => {
    sendMessage('Regenerate the previous response').catch(() => toast.error(t('chat.toast.regenerateFailed')))
  }

  if (isUser) {
    return (
      <div className="flex justify-end">
        <div className="max-w-[80%]">
          {isBranch && (
            <div className="flex items-center gap-xs mb-xs justify-end">
              <span className="material-symbols-outlined text-[14px] text-on-surface-variant/50">fork_right</span>
              <span className="font-label-sm text-on-surface-variant/50">{t('chat.message.branch')}</span>
            </div>
          )}
          <div className="bg-primary-fixed text-on-primary-fixed px-lg py-md rounded-2xl rounded-tr-none shadow-sm">
            <p className="font-body-md whitespace-pre-wrap">{message.content}</p>
          </div>
        </div>
      </div>
    )
  }

  return (
    <div className="flex gap-md max-w-[90%]">
      <div className="h-10 w-10 rounded-full bg-primary-container flex items-center justify-center shrink-0 shadow-md">
        <span className="material-symbols-outlined text-on-primary-container">smart_toy</span>
      </div>
      <div className="space-y-md flex-1">
        <div className="bg-surface-container-lowest px-lg py-md rounded-2xl rounded-tl-none border border-outline-variant/20 shadow-sm">
          <div className="font-body-md text-on-surface prose prose-sm max-w-none prose-p:my-1 prose-pre:bg-surface-container prose-pre:p-md prose-pre:rounded-lg prose-code:text-primary prose-code:before:content-[''] prose-code:after:content-['']">
            <ReactMarkdown remarkPlugins={[remarkGfm]} rehypePlugins={[rehypeHighlight]}>{message.content}</ReactMarkdown>
          </div>
          {message.tool_calls && message.tool_calls.length > 0 && (
            <div className="mt-md space-y-sm">
              {message.tool_calls.map(tc => (
                <ToolCallDisplay key={tc.tool_use_id} toolCall={tc} onViewDiff={onViewDiff} />
              ))}
            </div>
          )}
        </div>
        <div className="flex gap-sm">
          <Button aria-label={t('chat.message.like.aria')} aria-pressed={liked} onClick={() => setLiked(!liked)} className={`flex items-center gap-xs px-sm py-xs rounded-lg hover:bg-surface-container transition-colors ${liked ? 'text-primary' : 'text-on-surface-variant'}`}>
            <span className="material-symbols-outlined text-[18px]" aria-hidden="true">{liked ? 'thumb_up' : 'thumb_up_off_alt'}</span>
          </Button>
          <Button aria-label={t('chat.message.copy.aria')} onClick={handleCopy} className="flex items-center gap-xs px-sm py-xs rounded-lg hover:bg-surface-container text-on-surface-variant transition-colors">
            <span className="material-symbols-outlined text-[18px]" aria-hidden="true">content_copy</span>
          </Button>
          <Button aria-label={t('chat.message.regenerate.aria')} onClick={handleRegenerate} className="flex items-center gap-xs px-sm py-xs rounded-lg hover:bg-surface-container text-on-surface-variant transition-colors">
            <span className="material-symbols-outlined text-[18px]" aria-hidden="true">refresh</span>
          </Button>
        </div>
      </div>
    </div>
  )
});

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

const FILE_MUTATING_TOOLS = new Set(['write_file', 'edit_file', 'apply_patch', 'str_replace_editor', 'replace'])

function extractFilePath(toolName: string, input: unknown): string | null {
  if (!input || typeof input !== 'object') return null
  const obj = input as Record<string, unknown>
  const raw = typeof obj.path === 'string' ? obj.path
    : typeof obj.file_path === 'string' ? obj.file_path
    : typeof obj.filePath === 'string' ? obj.filePath
    : null
  if (!raw) return null
  return FILE_MUTATING_TOOLS.has(toolName) ? raw : null
}

const ToolCallDisplay = memo(function ToolCallDisplay({ toolCall, onViewDiff }: { toolCall: ToolCall; onViewDiff: (path: string) => void }) {
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })
  const [expanded, setExpanded] = useState(false)
  const statusIcon = toolCall.status === 'running' ? 'hourglass_empty' : toolCall.status === 'error' ? 'error' : 'check_circle'
  const statusColor = toolCall.status === 'running' ? 'text-secondary' : toolCall.status === 'error' ? 'text-error' : 'text-tertiary'
  const filePath = extractFilePath(toolCall.tool_name, toolCall.tool_input)
  const canDiff = filePath != null && toolCall.status === 'completed' && !toolCall.is_error

  return (
    <div className="p-sm bg-surface-container-low rounded-xl border border-outline-variant/10">
      <div className="flex items-center gap-sm cursor-pointer" onClick={() => setExpanded(!expanded)}>
        <span className={`material-symbols-outlined text-[16px] ${statusColor} ${toolCall.status === 'running' ? 'animate-spin' : ''}`}>{statusIcon}</span>
        <span className="font-label-md text-on-surface flex-1 truncate">{toolCall.tool_name}</span>
        {canDiff && (
          <button
            type="button"
            aria-label={intl.formatMessage({ id: 'chat.message.diff.aria' }, { path: filePath })}
            className="flex items-center gap-xs px-xs py-[2px] rounded-md text-tertiary hover:bg-tertiary-container/40 text-[11px] cursor-pointer"
            onClick={(e) => { e.stopPropagation(); onViewDiff(filePath!) }}
          >
            <span className="material-symbols-outlined text-[14px]">difference</span>
            {t('chat.message.diff')}
          </button>
        )}
        <span className="material-symbols-outlined text-[16px] text-on-surface-variant">{expanded ? 'expand_less' : 'expand_more'}</span>
      </div>
      {expanded && (
        <div className="mt-sm space-y-xs">
          {toolCall.tool_input ? (
            <pre className="text-body-sm text-on-surface-variant bg-surface-container p-sm rounded-lg overflow-x-auto max-h-[200px]">{JSON.stringify(toolCall.tool_input ?? null, null, 2)}</pre>
          ) : null}
          {toolCall.result && (
            toolCall.is_error ? (
              <pre className="text-body-sm p-sm rounded-lg overflow-x-auto max-h-[200px] bg-error/5 text-error">{toolCall.result}</pre>
            ) : (
              <div className="text-body-sm p-sm rounded-lg overflow-x-auto max-h-[200px] bg-surface-container text-on-surface-variant prose prose-sm max-w-none prose-pre:bg-surface-container-lowest prose-pre:p-sm prose-pre:rounded prose-code:text-primary prose-code:before:content-[''] prose-code:after:content-['']">
                <ReactMarkdown remarkPlugins={[remarkGfm]} rehypePlugins={[rehypeHighlight]}>{toolCall.result}</ReactMarkdown>
              </div>
            )
          )}
        </div>
      )}
    </div>
  )
});
