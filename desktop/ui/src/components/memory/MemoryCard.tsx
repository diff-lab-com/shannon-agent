// Memory panel — single memory row card (icon + metadata + content + tags +
// provenance badge + edit/delete actions). Extracted from MemoryPanel.tsx
// (T3.1); P2-4 adds the optional "source session" badge + jump.
import { useIntl } from 'react-intl'
import { Button } from '@/components/ui/button'
import type { MemoryEntry } from '@/lib/tauri-api'
import { CATEGORY_COLOR, CATEGORY_ICON } from './constants'
import { cn } from '@/lib/utils'

interface MemoryCardProps {
  entry: MemoryEntry
  onEdit: () => void
  onDelete: () => void
  onOpenMemorySource?: (memoryId: string, sourceSessionId: string) => void
}

function shortSession(id: string): string {
  return id.length > 10 ? `${id.slice(0, 8)}…` : id
}

export function MemoryCard({ entry, onEdit, onDelete, onOpenMemorySource }: MemoryCardProps) {
  const intl = useIntl()
  const t = (id: string, values?: Record<string, string | number>) =>
    intl.formatMessage({ id }, values)
  const fmtDate = (iso: string) => {
    const d = new Date(iso)
    if (Number.isNaN(d.getTime())) return iso
    return intl.formatDate(d, { year: 'numeric', month: 'short', day: 'numeric' })
  }

  return (
    <div className="px-md py-md rounded-xl bg-surface-container-low border border-outline-variant/30 shadow-sm hover:shadow-md hover:border-primary/30 transition-all">
      <div className="flex items-start gap-md">
        <span
          className={cn('material-symbols-outlined icon-md mt-[2px] px-sm py-xs rounded-lg', CATEGORY_COLOR[entry.category])}
        >
          {CATEGORY_ICON[entry.category]}
        </span>
        <div className="flex-1 min-w-0">
          <div className="flex items-center gap-sm mb-xs flex-wrap">
            <span className="text-label-xs px-sm py-[2px] rounded-full bg-surface-container-high text-on-surface-variant font-bold uppercase">
              {t(`memory.category.${entry.category}`)}
            </span>
            <span className="text-label-xs text-on-surface-variant">{entry.project}</span>
            <span className="text-label-xs text-on-surface-variant">
              · {fmtDate(entry.created_at)}
            </span>
            {entry.access_count > 0 && (
              <span className="text-label-xs text-on-surface-variant">
                · {intl.formatMessage({ id: 'memory.used' }, { count: entry.access_count })}
              </span>
            )}
            {/* P2-4 provenance: badge (with source kind) shown only when the
                entry carries a source session id, per the frozen UX contract. */}
            {entry.source_session_id && (
              <span
                className="inline-flex items-center gap-xs text-label-xs px-sm py-[2px] rounded-full bg-primary-container/40 text-on-surface"
                data-testid="memory-source-badge"
              >
                <span className="material-symbols-outlined text-[12px]" aria-hidden>
                  history
                </span>
                {entry.source_kind ? t(`memory.source.kind.${entry.source_kind}`) : t('memory.source.badge', { session: shortSession(entry.source_session_id) })}
                {entry.source_kind ? ` · ${shortSession(entry.source_session_id)}` : ''}
              </span>
            )}
          </div>
          <p className="text-body-md text-on-surface whitespace-pre-wrap break-words mb-md">
            {entry.content}
          </p>
          <div className="flex items-center flex-wrap gap-sm">
            {entry.tags.length > 0 && (
              <div className="flex flex-wrap gap-xs">
                {entry.tags.map((tag) => (
                  <span
                    key={tag}
                    className="text-label-xs px-sm py-[2px] rounded bg-primary-container text-on-primary-container"
                  >
                    #{tag}
                  </span>
                ))}
              </div>
            )}
            {entry.source_session_id && onOpenMemorySource && (
              <Button
                variant="outline"
                size="sm"
                className="gap-xs px-sm py-xs text-label-sm"
                onClick={() =>
                  onOpenMemorySource(entry.id, entry.source_session_id as string)
                }
              >
                <span className="material-symbols-outlined text-[14px]">chat</span>
                {t('memory.source.jump')}
              </Button>
            )}
          </div>
        </div>
        <div className="flex gap-xs">
          <Button
            variant="ghost"
            size="icon-sm"
            onClick={onEdit}
            aria-label={t('memory.action.edit')}
            className="rounded-lg hover:bg-surface-container-high"
          >
            <span className="material-symbols-outlined text-[18px] text-on-surface-variant">edit</span>
          </Button>
          <Button
            variant="ghost"
            size="icon-sm"
            onClick={onDelete}
            aria-label={t('memory.action.delete')}
            className="rounded-lg hover:bg-error/10"
          >
            <span className="material-symbols-outlined text-[18px] text-error/70">delete</span>
          </Button>
        </div>
      </div>
    </div>
  )
}
