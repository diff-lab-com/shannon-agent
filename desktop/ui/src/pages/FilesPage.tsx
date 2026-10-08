// FilesPage — office Wave 2 B9': the reference-style file library.
//
// Every file the user attached or the agent produced is indexed Rust-side
// (register_file_index_entry fires from the composer's attach flow and from
// FileCard mounts). This page lists that index newest-first with:
//   * favorite star toggle (persisted via set_file_index_favorite),
//   * missing detection via the existing path_exists probe (moved/deleted
//     cells grey out with the office.files.missing badge — the index keeps
//     the reference, it does not pretend the file is still there),
//   * open / reveal actions reusing FileCard's OS wrappers,
//   * the five design lenses (design 11-files.html): 全部 / 附件 /
//     Agent 产出 / 收藏 / 已丢失 — source tags and the missing probe are
//     both frontend-owned, so the extra lenses are pure client filtering.
//
// Layout parity (audit §11 P1): the rows render as a responsive file GRID
// (`repeat(auto-fill, minmax(160px, 1fr))` cells) instead of a single
// narrow list. The 320px preview panel is NOT built — it needs the
// session_id contract (audit C7) to source "来源会话" links, so it waits.
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

type FilesFilter = 'all' | 'attachments' | 'generated' | 'favorites' | 'missing'

/** File-type glyph per extension (coarse, icon-only — no pretense of a real
 *  preview; the i18n-free glyph keeps the cell language-neutral). */
function fileTypeIcon(name: string): string {
  const ext = name.split('.').pop()?.toLowerCase() ?? ''
  if (['png', 'jpg', 'jpeg', 'gif', 'webp', 'svg', 'bmp', 'ico'].includes(ext)) return 'image'
  if (ext === 'pdf') return 'picture_as_pdf'
  if (['md', 'txt', 'rst', 'log'].includes(ext)) return 'description'
  if (['zip', 'tar', 'gz', '7z', 'rar'].includes(ext)) return 'folder_zip'
  if (['json', 'toml', 'yaml', 'yml'].includes(ext)) return 'data_object'
  if (['ts', 'tsx', 'js', 'jsx', 'py', 'rs', 'go', 'java', 'c', 'cpp', 'sh'].includes(ext)) return 'code'
  return 'draft'
}

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

  // Per-lens counts stay live even while another lens is active (the
  // missing count trails its probes by design — it is a detection result,
  // not a given).
  const counts = useMemo(() => {
    const rows = entries ?? []
    return {
      all: rows.length,
      attachments: rows.filter(e => e.source === 'attachment').length,
      generated: rows.filter(e => e.source === 'generated').length,
      favorites: rows.filter(e => e.favorite).length,
      missing: rows.filter(e => missing[e.path] === true).length,
    }
  }, [entries, missing])

  const visible = useMemo(() => {
    if (!entries) return null
    switch (filter) {
      case 'attachments': return entries.filter(e => e.source === 'attachment')
      case 'generated': return entries.filter(e => e.source === 'generated')
      case 'favorites': return entries.filter(e => e.favorite)
      case 'missing': return entries.filter(e => missing[e.path] === true)
      default: return entries
    }
  }, [entries, filter, missing])

  const FILTERS: Array<{ key: FilesFilter; label: string }> = [
    { key: 'all', label: t('office.files.filter.all') },
    { key: 'attachments', label: t('office.files.filter.attachments') },
    { key: 'generated', label: t('office.files.filter.generated') },
    { key: 'favorites', label: t('office.files.filter.favorites') },
    { key: 'missing', label: t('office.files.filter.missing') },
  ]

  return (
    <div className="flex-1 overflow-y-auto">
      <div className="max-w-5xl mx-auto px-lg py-lg flex flex-col gap-md">
        <h1 className="font-headline-md text-headline-sm text-on-surface font-bold">{t('office.files.title')}</h1>

        {/* 五支筛选 pills (design 11-files.html:163) — counts ride along so
            each lens shows its own population up front. */}
        <div role="group" aria-label={t('office.files.title')} className="flex items-center gap-xs flex-wrap">
          {FILTERS.map(({ key, label }) => (
            <Button
              key={key}
              variant="ghost"
              size="sm"
              aria-pressed={filter === key}
              onClick={() => setFilter(key)}
              className={cn(
                'rounded-full font-label-sm gap-xs',
                filter === key
                  ? 'bg-primary-container text-on-primary-container'
                  : 'text-on-surface-variant hover:text-primary hover:bg-surface-container',
              )}
            >
              {key === 'favorites' && (
                <span
                  className="material-symbols-outlined icon-sm"
                  style={filter === 'favorites' ? { fontVariationSettings: "'FILL' 1" } : undefined}
                  aria-hidden="true"
                >
                  star
                </span>
              )}
              {label}
              <span className="tabular-nums opacity-70">{counts[key]}</span>
            </Button>
          ))}
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
          <ul
            className="grid gap-sm [grid-template-columns:repeat(auto-fill,minmax(160px,1fr))]"
            data-testid="files-list"
          >
            {visible.map(entry => {
              const isMissing = missing[entry.path] === true
              return (
                <li
                  key={entry.path}
                  data-testid="files-row"
                  data-missing={isMissing || undefined}
                  title={entry.path}
                  className={cn(
                    'relative flex flex-col gap-xs rounded-xl border border-outline-variant/20 bg-surface-container-low p-sm transition-colors hover:bg-surface-container hover:border-primary/30',
                    isMissing && 'opacity-50',
                  )}
                >
                  {/* Favorite star — top-right corner of the cell. */}
                  <button
                    type="button"
                    aria-label={entry.favorite ? t('office.files.unfavorite') : t('office.files.favorite')}
                    aria-pressed={entry.favorite}
                    title={entry.favorite ? t('office.files.unfavorite') : t('office.files.favorite')}
                    onClick={() => toggleFavorite(entry)}
                    className="absolute right-1.5 top-1.5 p-xs rounded-full text-tertiary hover:bg-surface-container-high focus-visible:outline focus-visible:outline-2 focus-visible:outline-primary"
                  >
                    <span
                      className="material-symbols-outlined icon-sm"
                      style={entry.favorite ? { fontVariationSettings: "'FILL' 1" } : undefined}
                      aria-hidden="true"
                    >
                      star
                    </span>
                  </button>
                  {/* Thumb: file-type glyph, language-neutral. */}
                  <span className="material-symbols-outlined icon-2xl text-on-surface-variant" aria-hidden="true">
                    {fileTypeIcon(entry.name)}
                  </span>
                  <span className="block font-label-sm text-on-surface truncate pr-lg">{entry.name}</span>
                  <span className="flex flex-wrap items-center gap-xs font-label-xs text-on-surface-variant tabular-nums">
                    {entry.size_bytes != null && <span>{formatFileSize(entry.size_bytes)}</span>}
                    <span aria-hidden="true">·</span>
                    <span>{formatRelativeTime(new Date(entry.registered_at).getTime(), now, t)}</span>
                    {entry.source === 'generated' && (
                      <>
                        <span aria-hidden="true">·</span>
                        <span>{intl.formatMessage({ id: 'chat.artifact.fromDisk' })}</span>
                      </>
                    )}
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
                  <div className="flex items-center gap-xs mt-auto">
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
