// Memory panel — single memory row card (icon + metadata + content + tags +
// edit/delete actions). Extracted from MemoryPanel.tsx (T3.1).
import { useIntl } from 'react-intl'
import { Button } from '@/components/ui/button'
import type { MemoryEntry } from '@/lib/tauri-api'
import { CATEGORY_COLOR, CATEGORY_ICON } from './constants'
import { cn } from '@/lib/utils'

interface MemoryCardProps {
  entry: MemoryEntry
  onEdit: () => void
  onDelete: () => void
}

export function MemoryCard({ entry, onEdit, onDelete }: MemoryCardProps) {
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })
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
          <div className="flex items-center gap-sm mb-xs">
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
          </div>
          <p className="text-body-md text-on-surface whitespace-pre-wrap break-words mb-md">
            {entry.content}
          </p>
          {entry.tags.length > 0 && (
            <div className="flex flex-wrap gap-xs mt-sm">
              {entry.tags.map((tag) => (
                <span
                  key={tag}
                  className="text-label-xs px-sm py-[2px] rounded bg-primary-container/30 text-on-primary-container"
                >
                  #{tag}
                </span>
              ))}
            </div>
          )}
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
