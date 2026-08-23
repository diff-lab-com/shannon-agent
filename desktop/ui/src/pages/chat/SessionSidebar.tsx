import type { IntlShape } from 'react-intl'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { ScrollArea } from '@/components/ui/scroll-area'
import { Pagination } from '@/components/ui/pagination'
import type { SessionInfo } from '@/types'
import { formatDirBreadcrumb, formatTime } from './utils'
import HighlightText from './HighlightText'

interface SessionSidebarProps {
  intl: IntlShape
  t: (id: string) => string
  sortedSessions: SessionInfo[]
  pagedSessions: SessionInfo[]
  sessionSearch: string
  setSessionSearch: (q: string) => void
  sessionPage: number
  sessionTotalPages: number
  setSessionPage: (page: number) => void
  currentSessionId: string | null
  pinnedIds: Set<string>
  editingSessionId: string | null
  editTitle: string
  setEditTitle: (s: string) => void
  createSession: () => Promise<void>
  switchSession: (id: string) => Promise<void>
  renameSession: (id: string, title: string) => Promise<void>
  setEditingSessionId: (id: string | null) => void
  setDeleteTarget: (id: string | null) => void
  togglePin: (id: string) => void
  handleExport: (id: string) => Promise<void>
  handlePrint: (id: string) => Promise<void>
}

export default function SessionSidebar({
  intl,
  t,
  sortedSessions,
  pagedSessions,
  sessionSearch,
  setSessionSearch,
  sessionPage,
  sessionTotalPages,
  setSessionPage,
  currentSessionId,
  pinnedIds,
  editingSessionId,
  editTitle,
  setEditTitle,
  createSession,
  switchSession,
  renameSession,
  setEditingSessionId,
  setDeleteTarget,
  togglePin,
  handleExport,
  handlePrint,
}: SessionSidebarProps) {
  const untitled = t('chat.session.untitled')
  return (
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
        {sortedSessions.length === 0 && (
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
                    <Button variant="ghost" size="icon-xs" className="rounded hover:bg-surface-container text-on-surface-variant hover:text-primary focus-visible:ring-2 focus-visible:ring-primary/30 focus-visible:outline-none" onClick={e => { e.stopPropagation(); togglePin(session.id) }} title={pinnedIds.has(session.id) ? t('chat.session.unpin') : t('chat.session.pin')} aria-pressed={pinnedIds.has(session.id)}>
                      <span className="material-symbols-outlined text-[14px]">{pinnedIds.has(session.id) ? 'push_pin' : 'keep'}</span>
                    </Button>
                    <Button variant="ghost" size="icon-xs" className="rounded hover:bg-surface-container text-on-surface-variant hover:text-primary focus-visible:ring-2 focus-visible:ring-primary/30 focus-visible:outline-none" onClick={e => { e.stopPropagation(); handleExport(session.id) }} title={t('chat.session.export')} aria-label={intl.formatMessage({ id: 'chat.session.export.aria' }, { title: session.title || untitled })}>
                      <span className="material-symbols-outlined text-[14px]">download</span>
                    </Button>
                    <Button variant="ghost" size="icon-xs" className="rounded hover:bg-surface-container text-on-surface-variant hover:text-primary focus-visible:ring-2 focus-visible:ring-primary/30 focus-visible:outline-none" onClick={e => { e.stopPropagation(); handlePrint(session.id) }} title={t('chat.session.print')} aria-label={intl.formatMessage({ id: 'chat.session.print.aria' }, { title: session.title || untitled })}>
                      <span className="material-symbols-outlined text-[14px]">print</span>
                    </Button>
                  </div>
                </div>
                <p className="text-body-sm text-on-surface-variant opacity-70 truncate">
                  {intl.formatMessage({ id: 'chat.session.meta' }, { count: session.message_count, time: formatTime(t, session.created_at) })}
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
  )
}