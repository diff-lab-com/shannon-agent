// FilesPage — office Wave 2 B9': the reference-style file library.
//
// Every file the user attached or the agent produced is indexed Rust-side
// (register_file_index_entry fires from the composer's attach flow and from
// FileCard mounts). This page lists that index newest-first with:
//   * favorite star toggle (persisted via set_file_index_favorite),
//   * missing detection via the existing path_exists probe (moved/deleted
//     cells carry the amber dashed lost form with the office.files.missing
//     badge — the index keeps the reference, it does not pretend the file
//     is still there),
//   * open / reveal actions reusing FileCard's OS wrappers,
//   * the five design lenses (design 11-files.html): 全部 / 附件 /
//     Agent 产出 / 收藏 / 已丢失 — source tags and the missing probe are
//     both frontend-owned, so the extra lenses are pure client filtering.
//
// Layout parity (audit §11 P1): the rows render as a responsive file GRID
// (`repeat(auto-fill, minmax(160px, 1fr))` cells) instead of a single
// narrow list.
//
// R2 Wave C′ (design 11 来源回跳): rows whose index entry carries a
// `session_id` (stamped backend-side at registration; historical rows have
// none) grow a 「来源会话」 jump — the MemoryCard provenance pattern:
// switchSession(sessionId) then navigate('/chat'). Absent stays absent.
//
// R2 Wave B1 (design 11 320px 预览面板): the grid becomes SELECTABLE.
// At ≥1280px (the design's two-column breakpoint) the page splits into
// grid + a fixed 320px preview card: type icon (extension-grouped),
// name, size, registration time, the 「来源会话」 jump and the extraction
// badge (`extracted_segments`, honesty contract: absent field → absent
// badge). The per-cell open/reveal/source buttons RETRACT into the panel
// at that width — the cell keeps only the favorite star (the design mock
// keeps the star in the cell too) plus the select hit-area. Below 1280px
// the panel does not render and the cells fall back to the inline action
// row (the pre-B1 form), so nothing is unreachable on a narrow window.
// Selection toggles: click the same cell again, press Esc, or hit the
// panel's close button. The breakpoint is JS-driven (window.matchMedia,
// same pattern as Layout's mobile branch) so tests can pin either mode.
//
// R2 final sweep (design 11:133-144): the pagehead grows the design's
// three pieces — the H1, its muted subtitle (11:138 「会话附件与 agent
// 产出的统一索引，可预览、可溯源」) and the actions-area search box
// (11:141-144, 220px, 「搜索文件名…」). The query filters by name/path,
// stacks with the lens pills (intersection), and the pill counts ride
// along so every lens shows its searched population. Esc in the input
// clears it. The 来源会话 jump renders the joined readable session title
// (`session_title`, joined backend-side at list time) and falls back to
// the generic label when the join has nothing readable.

// The page owns no state of the files themselves — it is a VIEW over
// references, consistent with the trust theme: nothing here sends anything.

import { useCallback, useContext, useEffect, useMemo, useState } from 'react'
import { useIntl } from 'react-intl'
import { useNavigate } from 'react-router-dom'
import { useT } from '@/i18n'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import EmptyState from '@/components/ui/empty-state'
import LoadingState from '@/components/ui/loading-state'
import { formatRelativeTime } from '@/components/SidebarSessions'
import { toastError } from '@/lib/errorToast'
import { formatFileSize } from '@/components/chat/FileCard'
import * as api from '@/lib/tauri-api'
import { SessionContext } from '@/context/SessionContext'
import { cn } from '@/lib/utils'
import type { FileIndexEntry } from '@/types'

type FilesFilter = 'all' | 'attachments' | 'generated' | 'favorites' | 'missing'

/** ≥1280px = the design's two-column breakpoint (grid + 320px preview). */
const WIDE_QUERY = '(min-width: 1280px)'

/** Coarse extension groups (design 11: 文档/表格/图片/代码/归档/…) driving
 *  both the cell glyph and the preview card's type label. */
type FileKind =
  | 'image' | 'video' | 'audio' | 'archive' | 'sheet' | 'slides'
  | 'pdf' | 'code' | 'data' | 'doc' | 'file'

const KIND_ICONS: Record<FileKind, string> = {
  image: 'image',
  video: 'play_circle',
  audio: 'music_note',
  archive: 'folder_zip',
  sheet: 'table_chart',
  slides: 'slideshow',
  pdf: 'picture_as_pdf',
  code: 'code',
  data: 'data_object',
  doc: 'description',
  file: 'draft',
}

const KIND_LABEL_KEYS: Record<FileKind, string> = {
  image: 'office.files.kind.image',
  video: 'office.files.kind.video',
  audio: 'office.files.kind.audio',
  archive: 'office.files.kind.archive',
  sheet: 'office.files.kind.sheet',
  slides: 'office.files.kind.slides',
  pdf: 'office.files.kind.pdf',
  code: 'office.files.kind.code',
  data: 'office.files.kind.data',
  doc: 'office.files.kind.doc',
  file: 'office.files.kind.file',
}

function fileKind(name: string): FileKind {
  const ext = name.split('.').pop()?.toLowerCase() ?? ''
  if (['png', 'jpg', 'jpeg', 'gif', 'webp', 'svg', 'bmp', 'ico', 'avif'].includes(ext)) return 'image'
  if (['mp4', 'mov', 'webm', 'avi', 'mkv'].includes(ext)) return 'video'
  if (['mp3', 'wav', 'flac', 'ogg', 'm4a'].includes(ext)) return 'audio'
  if (['zip', 'tar', 'gz', 'tgz', '7z', 'rar', 'bz2', 'xz'].includes(ext)) return 'archive'
  if (['csv', 'tsv', 'xls', 'xlsx', 'numbers'].includes(ext)) return 'sheet'
  if (['ppt', 'pptx', 'key'].includes(ext)) return 'slides'
  if (ext === 'pdf') return 'pdf'
  if (['ts', 'tsx', 'js', 'jsx', 'py', 'rs', 'go', 'java', 'c', 'cpp', 'h', 'sh', 'rb', 'php', 'kt', 'swift', 'html', 'css'].includes(ext)) return 'code'
  if (['json', 'toml', 'yaml', 'yml', 'xml'].includes(ext)) return 'data'
  if (['md', 'txt', 'rst', 'log', 'doc', 'docx', 'odt', 'rtf'].includes(ext)) return 'doc'
  return 'file'
}

/** File-type glyph per extension (coarse, icon-only — no pretense of a real
 *  preview; the i18n-free glyph keeps the cell language-neutral). */
function fileTypeIcon(name: string): string {
  return KIND_ICONS[fileKind(name)]
}

function prefersWidePreview(): boolean {
  return typeof window !== 'undefined'
    && typeof window.matchMedia === 'function'
    && window.matchMedia(WIDE_QUERY).matches
}

/** FilesPage body. Route-level chrome (the Layout shell) is provided by App. */
export default function FilesPage() {
  const intl = useIntl()
  const t = useT()
  const navigate = useNavigate()
  const sessionCtx = useContext(SessionContext)
  const [entries, setEntries] = useState<FileIndexEntry[] | null>(null)
  const [missing, setMissing] = useState<Record<string, boolean>>({})
  const [filter, setFilter] = useState<FilesFilter>('all')
  // R2 final sweep (design 11:141-144): the pagehead search box. Client-side
  // name/path substring match — no URL sync, no persistence, by design (the
  // index is a view; the query is ephemeral).
  const [query, setQuery] = useState('')
  // `now` pinned per load so every row's relative label agrees.
  const [now, setNow] = useState(() => Date.now())
  // R2 B1: currently selected index entry (by path), driving the preview
  // card. null = nothing selected → the panel shows its placeholder.
  const [selectedPath, setSelectedPath] = useState<string | null>(null)
  // R2 B1: ≥1280px two-column mode. JS-driven (not a CSS-only `xl:` toggle)
  // because the fallback is structural — the inline cell actions only
  // render in the narrow form — and so the branch is unit-testable.
  const [widePreview, setWidePreview] = useState(prefersWidePreview)

  useEffect(() => {
    if (typeof window === 'undefined' || typeof window.matchMedia !== 'function') return
    const mq = window.matchMedia(WIDE_QUERY)
    const update = () => setWidePreview(mq.matches)
    update()
    mq.addEventListener('change', update)
    return () => mq.removeEventListener('change', update)
  }, [])

  // R2 B1: Esc dismisses the selection (same escape grammar as the
  // drawers/dialogs). Bound only while something is selected.
  useEffect(() => {
    if (!selectedPath) return
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') setSelectedPath(null)
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [selectedPath])

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

  // R2 Wave C′: provenance jump — same pattern as the Memory page's
  // handleOpenMemorySource (switchSession, then /chat). Only entries with a
  // backend-stamped session_id get the button; failures toast instead of
  // navigating.
  const jumpToSource = useCallback(
    async (entry: FileIndexEntry) => {
      if (!entry.session_id) return
      try {
        await sessionCtx?.switchSession(entry.session_id)
        navigate('/chat')
      } catch (err) {
        toastError(t('office.files.title'), err)
      }
    },
    [navigate, sessionCtx, t],
  )

  // R2 final sweep: the search predicate — trimmed, case-insensitive
  // substring over name OR path. An empty query lets every row through.
  const matchesQuery = useCallback(
    (entry: FileIndexEntry) => {
      const q = query.trim().toLowerCase()
      if (!q) return true
      return entry.name.toLowerCase().includes(q) || entry.path.toLowerCase().includes(q)
    },
    [query],
  )

  // Per-lens counts stay live even while another lens is active (the
  // missing count trails its probes by design — it is a detection result,
  // not a given). R2 final sweep: the counts answer to the search box too —
  // each pill shows its lens's searched population, so the numbers and the
  // visible grid never disagree.
  const counts = useMemo(() => {
    const rows = (entries ?? []).filter(matchesQuery)
    return {
      all: rows.length,
      attachments: rows.filter(e => e.source === 'attachment').length,
      generated: rows.filter(e => e.source === 'generated').length,
      favorites: rows.filter(e => e.favorite).length,
      missing: rows.filter(e => missing[e.path] === true).length,
    }
  }, [entries, missing, matchesQuery])

  const visible = useMemo(() => {
    if (!entries) return null
    const base = (() => {
      switch (filter) {
        case 'attachments': return entries.filter(e => e.source === 'attachment')
        case 'generated': return entries.filter(e => e.source === 'generated')
        case 'favorites': return entries.filter(e => e.favorite)
        case 'missing': return entries.filter(e => missing[e.path] === true)
        default: return entries
      }
    })()
    // Search stacks with the lens (intersection), never replaces it.
    return base.filter(matchesQuery)
  }, [entries, filter, missing, matchesQuery])

  // R2 final sweep (design 11 预览卡可读标题): the source jump spells the
  // joined session title when the backend could resolve one; otherwise it
  // keeps the generic 「来源会话」 label (honest absence).
  const sourceJumpLabel = useCallback(
    (entry: FileIndexEntry) => entry.session_title ?? t('office.files.sourceJump'),
    [t],
  )

  // Selected entry resolved against the live index — a selection whose row
  // has vanished falls back to the placeholder instead of a stale card.
  const selectedEntry = useMemo(
    () => entries?.find(e => e.path === selectedPath) ?? null,
    [entries, selectedPath],
  )
  const selectedMissing = selectedEntry != null && missing[selectedEntry.path] === true
  const toggleSelection = (entry: FileIndexEntry) =>
    setSelectedPath(cur => (cur === entry.path ? null : entry.path))

  const FILTERS: Array<{ key: FilesFilter; label: string }> = [
    { key: 'all', label: t('office.files.filter.all') },
    { key: 'attachments', label: t('office.files.filter.attachments') },
    { key: 'generated', label: t('office.files.filter.generated') },
    { key: 'favorites', label: t('office.files.filter.favorites') },
    { key: 'missing', label: t('office.files.filter.missing') },
  ]

  return (
    <div className="flex-1 overflow-y-auto">
      <div className={cn('mx-auto px-lg py-lg flex flex-col gap-md', widePreview ? 'max-w-6xl' : 'max-w-5xl')}>
        {/* R2 final sweep (design 11:133-144 pagehead): H1 + muted subtitle
            on the left, the actions-area search box on the right (220px,
            「搜索文件名…」); on narrow widths the rows stack. */}
        <div className="flex items-start justify-between gap-md flex-wrap">
          <div className="min-w-0">
            <h1 className="font-headline-md text-headline-sm text-on-surface font-bold">{t('office.files.title')}</h1>
            <p className="font-label-sm text-on-surface-variant mt-[2px]">{t('office.files.subtitle')}</p>
          </div>
          {/* Same pill search input as the Extensions hub — icon + borderless
              Input inside a rounded shell. Esc clears (and does not also
              deselect a preview card: the event stops here). */}
          <div className="flex items-center bg-surface-container-lowest/50 rounded-full px-md py-xs border border-outline-variant/30 w-full max-w-[220px] focus-within:border-primary/40 focus-within:ring-2 focus-within:ring-primary/20 transition-colors shrink-0">
            <span className="material-symbols-outlined text-on-surface-variant mr-sm icon-md" aria-hidden="true">search</span>
            <Input
              type="text"
              data-testid="files-search"
              aria-label={t('office.files.searchAria')}
              placeholder={t('office.files.searchPlaceholder')}
              value={query}
              onChange={e => setQuery(e.target.value)}
              onKeyDown={e => {
                if (e.key === 'Escape') {
                  e.stopPropagation()
                  setQuery('')
                }
              }}
              className="bg-transparent border-none outline-none focus:ring-0 text-label-md font-label-md w-full"
            />
          </div>
        </div>

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
          /* R2 B1: ≥1280px → design 11's `1fr 320px` two-column body; below
             that the panel drops out entirely and the grid runs full-width
             with its inline cell actions (the pre-B1 form). */
          <div className={cn('flex items-start gap-md', widePreview ? 'flex-row' : 'flex-col')}>
            <ul
              className="grid flex-1 min-w-0 gap-sm [grid-template-columns:repeat(auto-fill,minmax(160px,1fr))]"
              data-testid="files-list"
            >
              {visible.map(entry => {
                const isMissing = missing[entry.path] === true
                const isSelected = selectedPath === entry.path
                return (
                  <li
                    key={entry.path}
                    data-testid="files-row"
                    data-selected={isSelected || undefined}
                    data-missing={isMissing || undefined}
                    title={entry.path}
                    className={cn(
                      'relative flex flex-col gap-xs rounded-xl border p-sm transition-colors cursor-pointer',
                      // R3 §4.1 (design 11:189 整卡琥珀虚线): a lost file's
                      // identity sits on the WHOLE card — dashed amber border
                      // (border-warning/50 border-dashed, the design's
                      // rgba(245,184,61,.5) dashed outline) instead of the
                      // old grey+opacity dimming, so the content stays
                      // readable. Selection re-tints the same dashed outline
                      // (warning/70 + warning container) rather than
                      // replacing it with the primary form.
                      isSelected
                        ? isMissing
                          ? 'border-warning/70 border-dashed bg-warning-container/25'
                          : 'border-primary/60 bg-primary-container/25'
                        : isMissing
                          ? 'border-warning/50 border-dashed bg-surface-container-low hover:bg-surface-container'
                          : 'border-outline-variant/20 bg-surface-container-low hover:bg-surface-container hover:border-primary/30',
                    )}
                  >
                    {/* Favorite star — top-right corner of the cell. Kept in
                        the cell at BOTH widths (design 11 keeps it there);
                        it sits above the select button without nesting
                        (absolute > static paint order), so clicks never
                        toggle the selection. */}
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
                    {/* R2 B1: the cell's main area is the select hit-area —
                        click selects (wide: also refreshes the preview;
                        narrow: state kept for when the window grows).
                        Clicking the same cell again deselects. */}
                    <button
                      type="button"
                      data-testid="files-row-select"
                      aria-pressed={isSelected}
                      onClick={() => toggleSelection(entry)}
                      className="flex flex-col gap-xs text-left bg-transparent border-0 p-0 cursor-pointer rounded-lg focus-visible:outline focus-visible:outline-2 focus-visible:outline-primary"
                    >
                      {/* Thumb: file-type glyph, language-neutral. A lost
                          file tints it amber (design 11:190 color:var(--warn)),
                          matching the card's dashed amber identity. */}
                      <span
                        className={cn(
                          'material-symbols-outlined icon-2xl',
                          isMissing ? 'text-warning' : 'text-on-surface-variant',
                        )}
                        aria-hidden="true"
                      >
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
                            className="inline-flex items-center gap-xs px-sm py-[2px] rounded-lg border border-dashed border-warning/50 text-warning font-label-xs"
                          >
                            <span className="material-symbols-outlined icon-xs" aria-hidden="true">folder_off</span>
                            {t('office.files.missing')}
                          </span>
                        )}
                      </span>
                    </button>
                    {/* Narrow-width fallback only — at ≥1280px these three
                        actions live in the preview card (B1 回撤). */}
                    {!widePreview && (
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
                        {/* R2 Wave C′ (design 11 来源会话): provenance jump, only
                            when the backend stamped a session at registration —
                            historical rows render nothing (honest absence).
                            R2 final sweep: the label is the joined readable
                            session title, falling back to the generic
                            「来源会话」 when the join resolved nothing. */}
                        {entry.session_id && (
                          <Button
                            variant="ghost"
                            size="sm"
                            data-testid="files-row-source-jump"
                            aria-label={sourceJumpLabel(entry)}
                            title={sourceJumpLabel(entry)}
                            onClick={() => void jumpToSource(entry)}
                            className="ml-auto gap-xs text-label-xs text-on-surface-variant hover:text-primary hover:bg-surface-container min-w-0"
                          >
                            <span className="material-symbols-outlined icon-sm">chat</span>
                            <span className="truncate">{sourceJumpLabel(entry)}</span>
                          </Button>
                        )}
                      </div>
                    )}
                  </li>
                )
              })}
            </ul>

            {/* R2 B1: 320px preview card (design 11:163 `1fr 320px`). Wide
                mode only — below the breakpoint the aside is not rendered
                at all (the cells' inline actions take over), so the fallback
                is structural, not a hidden-but-present column. */}
            {widePreview && (
              <aside
                data-testid="files-preview-panel"
                aria-label={t('office.files.preview')}
                className="w-80 shrink-0 rounded-xl border border-outline-variant/20 bg-surface-container-low p-md flex flex-col gap-md"
              >
                {selectedEntry ? (
                  <div data-testid="files-preview-card" className="flex flex-col gap-md">
                    {/* Preview zone: type glyph + coarse kind label — an
                        honest placeholder, not a fake thumbnail. */}
                    <div className="relative">
                      <div className="h-28 rounded-lg border border-outline-variant/20 bg-surface-container-lowest flex flex-col items-center justify-center gap-xs text-on-surface-variant">
                        <span className="material-symbols-outlined icon-2xl" aria-hidden="true">
                          {fileTypeIcon(selectedEntry.name)}
                        </span>
                        <span className="font-label-xs">
                          {t(KIND_LABEL_KEYS[fileKind(selectedEntry.name)])}
                          <span aria-hidden="true"> · </span>
                          {t('office.files.preview')}
                        </span>
                      </div>
                      <Button
                        variant="ghost"
                        size="icon-sm"
                        data-testid="files-preview-close"
                        aria-label={t('office.files.previewClose')}
                        title={t('office.files.previewClose')}
                        onClick={() => setSelectedPath(null)}
                        className="absolute right-1 top-1 text-on-surface-variant hover:text-primary hover:bg-surface-container"
                      >
                        <span className="material-symbols-outlined icon-sm" aria-hidden="true">close</span>
                      </Button>
                    </div>

                    <div className="min-w-0">
                      <div className="font-label-md font-bold text-on-surface break-all">{selectedEntry.name}</div>
                      <div className="flex flex-wrap items-center gap-xs font-label-xs text-on-surface-variant tabular-nums mt-[2px]">
                        {selectedEntry.size_bytes != null && <span>{formatFileSize(selectedEntry.size_bytes)}</span>}
                        <span aria-hidden="true">·</span>
                        <span>{formatRelativeTime(new Date(selectedEntry.registered_at).getTime(), now, t)}</span>
                      </div>
                    </div>

                    {selectedMissing && (
                      <span
                        data-testid="files-preview-missing"
                        role="note"
                        className="self-start inline-flex items-center gap-xs px-sm py-[2px] rounded-lg border border-dashed border-warning/50 text-warning font-label-xs"
                      >
                        <span className="material-symbols-outlined icon-xs" aria-hidden="true">folder_off</span>
                        {t('office.files.missing')}
                      </span>
                    )}

                    {/* 来源会话 jump — moved up from the cell (B1 回撤).
                        Only for backend-stamped entries; absent stays absent.
                        R2 final sweep: the label prefers the joined readable
                        session title (the preview card's whole point is
                        provenance you can READ), generic label on absence. */}
                    {selectedEntry.session_id && (
                      <Button
                        variant="ghost"
                        size="sm"
                        data-testid="files-preview-source-jump"
                        aria-label={sourceJumpLabel(selectedEntry)}
                        title={sourceJumpLabel(selectedEntry)}
                        onClick={() => void jumpToSource(selectedEntry)}
                        className="self-start gap-xs text-label-sm text-primary hover:bg-surface-container min-w-0"
                      >
                        <span className="material-symbols-outlined icon-sm" aria-hidden="true">chat</span>
                        <span className="truncate">{sourceJumpLabel(selectedEntry)}</span>
                      </Button>
                    )}

                    {/* Extraction badge (R2 B2 contract): rendered only when
                        the index actually carries extracted_segments — no
                        field, no badge (honesty contract). */}
                    {selectedEntry.extracted_segments != null && (
                      <span
                        data-testid="files-preview-extracted"
                        className="self-start inline-flex items-center gap-xs px-sm py-[2px] rounded-lg bg-primary-container/60 text-on-primary-container font-label-xs"
                      >
                        <span className="material-symbols-outlined icon-xs" aria-hidden="true">check_circle</span>
                        {t('office.files.extracted', { count: selectedEntry.extracted_segments })}
                      </span>
                    )}

                    {/* The three cell actions, retracted here (B1). Open/
                        reveal stay disabled for a missing file — same rule
                        as the narrow cells. */}
                    <div className="flex items-center gap-xs flex-wrap mt-auto pt-xs">
                      <Button
                        variant="outline"
                        size="sm"
                        data-testid="files-preview-favorite"
                        aria-pressed={selectedEntry.favorite}
                        onClick={() => toggleFavorite(selectedEntry)}
                        className="gap-xs font-label-sm"
                      >
                        <span
                          className="material-symbols-outlined icon-sm"
                          style={selectedEntry.favorite ? { fontVariationSettings: "'FILL' 1" } : undefined}
                          aria-hidden="true"
                        >
                          star
                        </span>
                        {selectedEntry.favorite ? t('office.files.unfavorite') : t('office.files.favorite')}
                      </Button>
                      <Button
                        variant="outline"
                        size="sm"
                        data-testid="files-preview-open"
                        disabled={selectedMissing}
                        onClick={() => open(selectedEntry)}
                        className="gap-xs font-label-sm"
                      >
                        <span className="material-symbols-outlined icon-sm" aria-hidden="true">open_in_new</span>
                        {t('chat.message.attachment.open')}
                      </Button>
                      <Button
                        variant="outline"
                        size="sm"
                        data-testid="files-preview-reveal"
                        disabled={selectedMissing}
                        onClick={() => reveal(selectedEntry)}
                        className="gap-xs font-label-sm"
                      >
                        <span className="material-symbols-outlined icon-sm" aria-hidden="true">folder_open</span>
                        {t('chat.message.filecard.reveal')}
                      </Button>
                    </div>
                  </div>
                ) : (
                  <div data-testid="files-preview-empty" className="flex-1 flex items-center">
                    <EmptyState
                      compact
                      icon="touch_app"
                      title={t('office.files.previewEmpty')}
                      description={t('office.files.previewEmptyHint')}
                    />
                  </div>
                )}
              </aside>
            )}
          </div>
        )}
      </div>
    </div>
  )
}
