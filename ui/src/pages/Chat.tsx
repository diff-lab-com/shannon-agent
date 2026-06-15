import { useState, useRef, useEffect, memo } from 'react'
import ReactMarkdown from 'react-markdown'
import { toast } from 'sonner'
import remarkGfm from 'remark-gfm'
import rehypeHighlight from 'rehype-highlight'
import { open as openDialog } from '@tauri-apps/plugin-dialog'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { ScrollArea } from '@/components/ui/scroll-area'
import { Pagination } from '@/components/ui/pagination'
import WelcomeState from '@/components/WelcomeState'
import DiffDialog from '@/components/diff/DiffDialog'
import { useApp } from '@/context/AppContext'
import * as api from '@/lib/tauri-api'
import type { ChatMessage, ToolCall, FileContext } from '@/types'

export default function Chat() {
  const {
    messages, streamingText, thinkingText, isQuerying, activeToolCalls, usage,
    sessions, currentSessionId, error,
    sendMessage, cancelQuery, createSession, switchSession, deleteSession, renameSession,
  } = useApp()

  const [input, setInput] = useState('')
  const [sessionSearch, setSessionSearch] = useState('')
  const [editingSessionId, setEditingSessionId] = useState<string | null>(null)
  const [editTitle, setEditTitle] = useState('')
  const [diffPath, setDiffPath] = useState<string | null>(null)
  const [fileContext, setFileContext] = useState<FileContext[]>([])
  const [attachedFiles, setAttachedFiles] = useState<string[]>([])
  const [pinnedIds, setPinnedIds] = useState<Set<string>>(new Set())
  const [sessionPage, setSessionPage] = useState(1)
  const [deleteTarget, setDeleteTarget] = useState<string | null>(null)
  const messagesEndRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    messagesEndRef.current?.scrollIntoView({ behavior: 'smooth' })
  }, [messages, streamingText])

  useEffect(() => {
    api.getFileContext().then(setFileContext).catch(e => console.warn('Failed to load file context:', e))
  }, [messages])

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
          { name: 'Documents & Images', extensions: ['pdf', 'png', 'jpg', 'jpeg', 'gif', 'webp'] },
        ],
      })
      if (!selected) return
      const paths = (Array.isArray(selected) ? selected : [selected]) as string[]
      if (paths.length > 0) setAttachedFiles(prev => [...prev, ...paths])
    } catch (err) {
      toast.error('Attach failed', { description: String(err) })
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

  const filteredSessions = sessionSearch
    ? sessions.filter(s => s.title.toLowerCase().includes(sessionSearch.toLowerCase()))
    : sessions

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
      await api.exportSession(id, 'markdown')
      toast.success('Session exported')
    } catch (e) { console.warn('Export failed:', e); toast.error('Export failed') }
  }

  const formatTime = (ts: number) => {
    const d = new Date(ts)
    const now = new Date()
    if (d.toDateString() === now.toDateString()) return 'Today'
    const yesterday = new Date(now)
    yesterday.setDate(yesterday.getDate() - 1)
    if (d.toDateString() === yesterday.toDateString()) return 'Yesterday'
    return d.toLocaleDateString()
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
            New Chat
          </Button>
          <div className="relative mt-sm">
            <span className="material-symbols-outlined absolute left-sm top-1/2 -translate-y-1/2 text-on-surface-variant text-[18px]">search</span>
            <Input
              className="w-full pl-xl pr-md py-xs bg-surface-container border-none rounded-lg text-body-sm focus:ring-1 focus:ring-primary/30"
              placeholder="Search sessions..."
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
              <p className="text-body-sm text-on-surface-variant mt-xs">No sessions yet</p>
            </div>
          )}
          {pagedSessions.map(session => (
            <div
              key={session.id}
              role="button"
              tabIndex={0}
              aria-label={`Session: ${session.title || 'Untitled'}`}
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
                      <HighlightText text={session.title || 'Untitled'} query={sessionSearch} />
                    </p>
                    <div className="flex items-center gap-xs opacity-0 group-hover:opacity-100 transition-opacity shrink-0">
                      <button className="p-xs rounded hover:bg-surface-container text-on-surface-variant hover:text-primary focus-visible:ring-2 focus-visible:ring-primary/30 focus-visible:outline-none" onClick={e => { e.stopPropagation(); togglePin(session.id) }} title={pinnedIds.has(session.id) ? 'Unpin' : 'Pin'} aria-pressed={pinnedIds.has(session.id)}>
                        <span className="material-symbols-outlined text-[14px]">{pinnedIds.has(session.id) ? 'push_pin' : 'keep'}</span>
                      </button>
                      <button className="p-xs rounded hover:bg-surface-container text-on-surface-variant hover:text-primary focus-visible:ring-2 focus-visible:ring-primary/30 focus-visible:outline-none" onClick={e => { e.stopPropagation(); handleExport(session.id) }} title="Export">
                        <span className="material-symbols-outlined text-[14px]">download</span>
                      </button>
                    </div>
                  </div>
                  <p className="text-body-sm text-on-surface-variant opacity-70 truncate">
                    {session.message_count} messages · {formatTime(session.created_at)}
                  </p>
                </>
              )}
            </div>
          ))}
        </ScrollArea>
        <Pagination page={sessionPage} totalPages={sessionTotalPages} onPageChange={setSessionPage} />
      </aside>

      {/* Main Chat Canvas */}
      <section className="flex-1 flex flex-col relative bg-surface-container-lowest/40 overflow-hidden">
        {/* Message Area */}
        <ScrollArea className="flex-1 p-xl space-y-xl pb-32">
          {messages.length === 0 && !streamingText && (
            sessions.length === 0 ? (
              <WelcomeState onSelectPrompt={setInput} />
            ) : (
              <div className="flex items-center justify-center h-full opacity-40">
                <div className="text-center space-y-sm">
                  <span className="material-symbols-outlined text-[48px] text-primary">chat_bubble</span>
                  <p className="font-body-lg text-on-surface-variant">Start a conversation</p>
                </div>
              </div>
            )
          )}

          {messages.map((msg, i) => (
            <MessageBubble key={`${msg.timestamp}-${i}`} message={msg} onViewDiff={setDiffPath} />
          ))}

          {/* Streaming response */}
          {(streamingText || thinkingText || activeToolCalls.length > 0) && (
            <div className="flex gap-md max-w-[90%]" aria-live="polite" aria-label="AI response streaming">
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
                      <span className="font-label-sm text-on-surface-variant block uppercase opacity-70">Thinking</span>
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
              <Button variant="ghost" className="mt-sm text-error hover:bg-error/10 text-label-md cursor-pointer" onClick={() => { if (input.trim()) handleSend() }}>Retry</Button>
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
              <Button variant="ghost" aria-label="Attach file" className="p-md text-on-surface-variant hover:text-primary" onClick={handleAttach}>
                <span className="material-symbols-outlined text-[20px]" aria-hidden="true">attach_file</span>
              </Button>
              <span className="material-symbols-outlined p-md text-primary" aria-hidden="true">{isQuerying ? 'hourglass_empty' : 'auto_awesome'}</span>
              <textarea
                className="flex-1 bg-transparent border-none outline-none focus:ring-0 font-body-lg py-md px-sm placeholder:text-outline-variant/80 text-on-surface resize-none min-h-[24px] max-h-[200px]"
                placeholder={isQuerying ? 'Processing...' : 'Ask Shannon anything...'}
                value={input}
                onChange={e => setInput(e.target.value)}
                onKeyDown={handleKeyDown}
                rows={1}
                disabled={isQuerying}
              />
              <div className="flex items-center gap-2 px-sm">
                {isQuerying ? (
                  <Button aria-label="Stop generation" className="bg-error/80 text-on-error p-3 rounded-xl active:scale-95 transition-all" onClick={cancelQuery}>
                    <span className="material-symbols-outlined text-[20px]" aria-hidden="true">stop</span>
                  </Button>
                ) : (
                  <Button
                    aria-label="Send message"
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
              <h3 className="font-headline-md text-on-surface">Delete Session</h3>
            </div>
            <p className="text-body-md text-on-surface-variant mb-lg">Are you sure you want to delete this session? This cannot be undone.</p>
            <div className="flex justify-end gap-sm">
              <Button className="px-lg py-sm rounded-xl text-on-surface-variant hover:bg-surface-container" onClick={() => setDeleteTarget(null)}>Cancel</Button>
              <Button className="px-lg py-sm rounded-xl bg-error text-on-error hover:bg-error/90" onClick={() => { deleteSession(deleteTarget); setDeleteTarget(null) }}>Delete</Button>
            </div>
          </div>
        </div>
      )}

      {/* Right Sidebar - Context */}
      <aside aria-label="Context panel" className="w-[300px] border-l border-outline-variant/10 glass-panel shrink-0 p-lg overflow-y-auto bg-surface-container-lowest/50 hidden lg:block">
        <div className="space-y-xl">
          {/* Token Usage */}
          {usage && (
            <section>
              <h3 className="font-label-md text-on-surface uppercase tracking-wider opacity-60 mb-md">Usage</h3>
              <div className="p-md bg-surface-container rounded-xl border border-outline-variant/10 space-y-sm">
                <div className="flex justify-between text-body-sm">
                  <span className="text-on-surface-variant">Input tokens</span>
                  <span className="font-bold text-on-surface">{usage.input_tokens.toLocaleString()}</span>
                </div>
                <div className="flex justify-between text-body-sm">
                  <span className="text-on-surface-variant">Output tokens</span>
                  <span className="font-bold text-on-surface">{usage.output_tokens.toLocaleString()}</span>
                </div>
                <div className="flex justify-between text-body-sm">
                  <span className="text-on-surface-variant">Cost</span>
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
                        <span>Context Window</span>
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
                Active Tools
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
                Context Files
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
                      <span>{fc.lines} lines</span>
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

  const handleCopy = () => {
    navigator.clipboard.writeText(message.content).catch(() => toast.error('Copy failed'))
  }

  const handleRegenerate = () => {
    sendMessage('Regenerate the previous response').catch(() => toast.error('Regeneration failed'))
  }

  if (isUser) {
    return (
      <div className="flex justify-end">
        <div className="max-w-[80%]">
          {isBranch && (
            <div className="flex items-center gap-xs mb-xs justify-end">
              <span className="material-symbols-outlined text-[14px] text-on-surface-variant/50">fork_right</span>
              <span className="font-label-sm text-on-surface-variant/50">Edited branch</span>
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
          <Button aria-label="Like message" aria-pressed={liked} onClick={() => setLiked(!liked)} className={`flex items-center gap-xs px-sm py-xs rounded-lg hover:bg-surface-container transition-colors ${liked ? 'text-primary' : 'text-on-surface-variant'}`}>
            <span className="material-symbols-outlined text-[18px]" aria-hidden="true">{liked ? 'thumb_up' : 'thumb_up_off_alt'}</span>
          </Button>
          <Button aria-label="Copy message" onClick={handleCopy} className="flex items-center gap-xs px-sm py-xs rounded-lg hover:bg-surface-container text-on-surface-variant transition-colors">
            <span className="material-symbols-outlined text-[18px]" aria-hidden="true">content_copy</span>
          </Button>
          <Button aria-label="Regenerate response" onClick={handleRegenerate} className="flex items-center gap-xs px-sm py-xs rounded-lg hover:bg-surface-container text-on-surface-variant transition-colors">
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
            aria-label={`View diff for ${filePath}`}
            className="flex items-center gap-xs px-xs py-[2px] rounded-md text-tertiary hover:bg-tertiary-container/40 text-[11px] cursor-pointer"
            onClick={(e) => { e.stopPropagation(); onViewDiff(filePath!) }}
          >
            <span className="material-symbols-outlined text-[14px]">difference</span>
            Diff
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
