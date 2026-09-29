// FilesPage — office Wave 2 B9': the reference-style file library.
//
// Every file the user attached or the agent produced is indexed Rust-side
// (register_file_index_entry fires from the composer's attach flow and from
// FileCard mounts). This page lists that index newest-first with:
//   * favorite star toggle (persisted via set_file_index_favorite),
//   * missing detection via the existing path_exists probe (moved/deleted
//     rows grey out with the office.files.missing badge — the index keeps
//     the reference, it does not pretend the file is still there),
//   * open / reveal actions reusing FileCard's OS wrappers,
//   * an All / Favorites lens.
//
// The page owns no state of the files themselves — it is a VIEW over
// references, consistent with the trust theme: nothing here sends anything.

import { useEffect, useMemo, useState } from 'react'
import { useIntl } from 'react-intl'
import { useT } from '@/i18n'
import { Button } from '@/components/ui/button'
import EmptyState from '@/components/ui/empty-state'
import LoadingState from '@/components/ui/loading-state'
import { formatRelativeTime } from '@/components/SidebarSessions'
import { toastError } from '@/lib/errorToast'
import { formatFileSize } from '@/components/chat/FileCard'
import * as api from '@/lib/tauri-api'
import { cn } from '@/lib/utils'
import type { FileIndexEntry } from '@/types'

type FilesFilter = 'all' | 'favorites'

/** FilesPage body. Route-level chrome (the Layout shell) is provided by App. */
export default function FilesPage() {
  const intl = useIntl()
  const t = useT()
  const [entries, setEntries] = useState<FileIndexEntry[] | null>(null)
  const [missing, setMissing] = useState<Record<string, boolean>>({})
  const [filter, setFilter] = useState<FilesFilter>('all')
  // `now` pinned per load so every row's relative label agrees.
  const [now, setNow] = useState(() => Date.now())

  const load = () => {
    api
      .listFileIndex()
      .then(rows => {
        setEntries(rows)
        setNow(Date.now())
        // Missing detection — one probe per row, best-effort. A probe
        // failure leaves the row untainted (never claim "missing" on an
        // error; only on a confirmed false).
        for (const row of rows) {
          api
            .pathExists(row.path)
            .then(exists => { if (!exists) setMissing(m => ({ ...m, [row.path]: true })) })
            .catch(() => { /* probe unavailable — leave the row as-is */ })
        }
      })
      .catch(err => {
        setEntries([])
        toastError(t('office.files.title'), err)
      })
  }

  useEffect(() => {
    load()
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  const toggleFavorite = (entry: FileIndexEntry) => {
    const next = !entry.favorite
    // Optimistic flip; revert + toast on failure (demo mode without the
    // command, disk errors).
    setEntries(rows => rows?.map(r => (r.path === entry.path ? { ...r, favorite: next } : r)) ?? rows)
    api.setFileIndexFavorite(entry.path, next).catch(err => {
      setEntries(rows => rows?.map(r => (r.path === entry.path ? { ...r, favorite: !next } : r)) ?? rows)
      toastError(t('office.files.title'), err)
    })
  }

  const open = (entry: FileIndexEntry) => {
    api.openWithDefaultApp(entry.path).catch(err => toastError(t('link.open.failed'), err))
  }

  const reveal = (entry: FileIndexEntry) => {
    api.revealInFolder(entry.path).catch(err => toastError(t('link.open.failed'), err))
  }

  const visible = useMemo(() => {
    if (!entries) return null
    return filter === 'favorites' ? entries.filter(e => e.favorite) : entries
  }, [entries, filter])

  return (
    <div className="flex-1 overflow-y-auto">
      <div className="max-w-3xl mx-auto px-lg py-lg flex flex-col gap-md">
        <div className="flex items-center justify-between gap-sm">
          <h1 className="font-headline-md text-headline-sm text-on-surface font-bold">{t('office.files.title')}</h1>
          {/* All / Favorites lens — the Favorites leg is the star itself
              (office.files.favorite carries its accessible name). */}
          <div role="group" aria-label={t('office.files.title')} className="flex items-center gap-xs">
            <Button
              variant="ghost"
              size="sm"
              aria-pressed={filter === 'all'}
              onClick={() => setFilter('all')}
              className={cn(
                'rounded-lg font-label-sm',
                filter === 'all' ? 'bg-primary-container text-on-primary-container' : 'text-on-surface-variant hover:text-primary',
              )}
            >
              {t('inbox.filter.all')}
            </Button>
            <Button
              variant="ghost"
              size="sm"
              aria-pressed={filter === 'favorites'}
              aria-label={t('office.files.favorite')}
              title={t('office.files.favorite')}
              onClick={() => setFilter('favorites')}
              className={cn(
                'rounded-lg',
                filter === 'favorites' ? 'bg-primary-container text-on-primary-container' : 'text-on-surface-variant hover:text-primary',
              )}
            >
              <span
                className="material-symbols-outlined icon-sm"
                style={filter === 'favorites' ? { fontVariationSettings: "'FILL' 1" } : undefined}
                aria-hidden="true"
              >
                star
              </span>
            </Button>
          </div>
        </div>

        {visible == null ? (
          <LoadingState label={t('office.files.title')} />
        ) : visible.length === 0 ? (
          <EmptyState
            icon="folder_open"
            title={t('office.files.title')}
            description={t('office.files.empty')}
          />
        ) : (
          <ul className="flex flex-col gap-xs" data-testid="files-list">
            {visible.map(entry => {
              const isMissing = missing[entry.path] === true
              return (
                <li
                  key={entry.path}
                  data-testid="files-row"
                  data-missing={isMissing || undefined}
                  title={entry.path}
                  className={cn(
                    'flex items-center gap-sm rounded-lg border border-outline-variant/20 bg-surface-container-low px-sm py-xs transition-colors hover:bg-surface-container',
                    isMissing && 'opacity-50',
                  )}
                >
                  <button
                    type="button"
                    aria-label={entry.favorite ? t('office.files.unfavorite') : t('office.files.favorite')}
                    aria-pressed={entry.favorite}
                    title={entry.favorite ? t('office.files.unfavorite') : t('office.files.favorite')}
                    onClick={() => toggleFavorite(entry)}
                    className="shrink-0 p-xs rounded-full text-tertiary hover:bg-surface-container-high focus-visible:outline focus-visible:outline-2 focus-visible:outline-primary"
                  >
                    <span
                      className="material-symbols-outlined icon-sm"
                      style={entry.favorite ? { fontVariationSettings: "'FILL' 1" } : undefined}
                      aria-hidden="true"
                    >
                      star
                    </span>
                  </button>
                  <span className="material-symbols-outlined icon-sm text-on-surface-variant shrink-0" aria-hidden="true">draft</span>
                  <div className="min-w-0 flex-1">
                    <span className="block font-label-sm text-on-surface truncate">{entry.name}</span>
                    <span className="flex items-center gap-xs font-label-xs text-on-surface-variant tabular-nums">
                      {entry.size_bytes != null && <span>{formatFileSize(entry.size_bytes)}</span>}
                      <span aria-hidden="true">·</span>
                      <span>{formatRelativeTime(new Date(entry.registered_at).getTime(), now, t)}</span>
                      {entry.source === 'generated' && <span aria-hidden="true">·</span>}
                      {entry.source === 'generated' && <span>{intl.formatMessage({ id: 'chat.artifact.fromDisk' })}</span>}
                      {isMissing && (
                        <span
                          data-testid="files-row-missing"
                          role="note"
                          className="text-warning font-label-xs"
                        >
                          · {t('office.files.missing')}
                        </span>
                      )}
                    </span>
                  </div>
                  <div className="flex shrink-0 items-center gap-xs">
                    <Button
                      variant="ghost"
                      size="icon-sm"
                      disabled={isMissing}
                      aria-label={t('chat.message.attachment.open')}
                      title={t('chat.message.attachment.open')}
                      onClick={() => open(entry)}
                      className="text-on-surface-variant hover:text-primary hover:bg-surface-container"
                    >
                      <span className="material-symbols-outlined icon-sm">open_in_new</span>
                    </Button>
                    <Button
                      variant="ghost"
                      size="icon-sm"
                      disabled={isMissing}
                      aria-label={t('chat.message.filecard.reveal')}
                      title={t('chat.message.filecard.reveal')}
                      onClick={() => reveal(entry)}
                      className="text-on-surface-variant hover:text-primary hover:bg-surface-container"
                    >
                      <span className="material-symbols-outlined icon-sm">folder_open</span>
                    </Button>
                  </div>
                </li>
              )
            })}
          </ul>
        )}
      </div>
    </div>
  )
}
