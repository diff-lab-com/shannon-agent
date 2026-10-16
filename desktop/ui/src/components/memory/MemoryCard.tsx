// Memory panel — single memory row card (design 08 记忆行解剖, G4a): 32px
// category tile left, bold 13px/600 content title with the category chip
// riding after it, then the meta line = source-session link + time +
// 「注入 N 次」. Provenance: with a source session id the meta link jumps to
// the session (the page-owned switchSession flow); without one only the
// source-kind plain text shows (honest absence). Actions live in the right
// … menu (edit / delete, same handlers as before).
//
// Extracted from MemoryPanel.tsx (T3.1); P2-4 added the source badge + jump.
import { useState } from 'react'
import { useIntl } from 'react-intl'
import { DropdownMenu, type DropdownMenuItem } from '@/components/ui/dropdown-menu'
import type { MemoryEntry } from '@/lib/tauri-api'
import { CATEGORY_COLOR, CATEGORY_ICON } from './constants'
import { cn } from '@/lib/utils'

interface MemoryCardProps {
  entry: MemoryEntry
  onEdit: () => void
  onDelete: () => void
  onOpenMemorySource?: (memoryId: string, sourceSessionId: string) => void
}

export function MemoryCard({ entry, onEdit, onDelete, onOpenMemorySource }: MemoryCardProps) {
  const intl = useIntl()
  const t = (id: string, values?: Record<string, string | number>) =>
    intl.formatMessage({ id }, values)
  const [menuOpen, setMenuOpen] = useState(false)
  const fmtDate = (iso: string) => {
    const d = new Date(iso)
    if (Number.isNaN(d.getTime())) return iso
    return intl.formatDate(d, { year: 'numeric', month: 'short', day: 'numeric' })
  }

  const menuItems: DropdownMenuItem[] = [
    { id: 'edit', label: t('memory.action.edit'), icon: 'edit', onSelect: onEdit },
    {
      id: 'delete',
      label: t('memory.action.delete'),
      icon: 'delete',
      destructive: true,
      onSelect: onDelete,
    },
  ]

  return (
    <div
      className="px-md py-md rounded-xl bg-surface-container-low border border-outline-variant/30 shadow-e1 hover:shadow-e2 hover:border-primary/30 transition-all"
      data-testid="memory-card"
    >
      <div className="flex items-start gap-md">
        {/* G4a (design 08:332-338): 32px rounded category tile, semantic dim
            pair, thin-line icon. */}
        <span
          className={cn(
            'w-8 h-8 rounded-lg flex items-center justify-center shrink-0',
            CATEGORY_COLOR[entry.category],
          )}
          aria-hidden="true"
        >
          <span className="material-symbols-outlined icon-md">{CATEGORY_ICON[entry.category]}</span>
        </span>
        <div className="flex-1 min-w-0">
          {/* Content first, bold (13px/600 = text-label-lg), the category
              chip rides after the title (design 08:341-343). */}
          <div className="flex items-start gap-sm flex-wrap">
            <p className="text-label-lg font-semibold text-on-surface whitespace-pre-wrap break-words min-w-0 flex-1">
              {entry.content}
            </p>
            <span
              className={cn(
                'text-label-xs px-sm py-[2px] rounded-full font-bold shrink-0',
                CATEGORY_COLOR[entry.category],
              )}
            >
              {t(`memory.category.${entry.category}`)}
            </span>
          </div>

          {/* Meta line (design 08:345): 来源会话 link + time + 注入 N 次. */}
          <div className="flex items-center flex-wrap gap-x-sm gap-y-xs mt-xs text-label-xs text-on-surface-variant">
            {entry.source_session_id && onOpenMemorySource ? (
              <button
                type="button"
                data-testid="memory-source-link"
                aria-label={t('memory.source.jump')}
                onClick={() => onOpenMemorySource(entry.id, entry.source_session_id as string)}
                className="inline-flex items-center gap-xs text-link hover:underline cursor-pointer focus-visible:outline focus-visible:outline-2 focus-visible:outline-primary rounded-sm min-w-0"
              >
                <span className="material-symbols-outlined icon-xs" aria-hidden="true">chat</span>
                <span className="truncate">{entry.source_kind ? t(`memory.source.kind.${entry.source_kind}`) : t('memory.source.jump')}</span>
              </button>
            ) : entry.source_kind ? (
              <span data-testid="memory-source-badge" className="inline-flex items-center gap-xs min-w-0">
                <span className="material-symbols-outlined icon-xs" aria-hidden="true">history</span>
                <span className="truncate">{t(`memory.source.kind.${entry.source_kind}`)}</span>
              </span>
            ) : null}
            <span className="tabular-nums">{fmtDate(entry.created_at)}</span>
            {entry.access_count > 0 && (
              <span data-testid="memory-injected-count" className="tabular-nums">
                {t('memory.injected', { count: entry.access_count })}
              </span>
            )}
          </div>

          {entry.tags.length > 0 && (
            <div className="flex flex-wrap gap-xs mt-sm">
              {entry.tags.map((tag) => (
                <span
                  key={tag}
                  className="text-label-xs px-sm py-[2px] rounded-sm bg-primary-container text-on-primary-container"
                >
                  #{tag}
                </span>
              ))}
            </div>
          )}
        </div>

        {/* G4a (design 08:348): the row's … menu — edit / delete, same
            handlers as the old inline icon pair. The trigger needs the open
            state so the menu can anchor here. */}
        <span className="relative shrink-0">
          <button
            type="button"
            aria-label={t('memory.card.menu.aria')}
            aria-haspopup="menu"
            aria-expanded={menuOpen}
            data-testid="memory-card-menu"
            onClick={() => setMenuOpen(v => !v)}
            className="inline-flex items-center justify-center w-8 h-8 rounded-lg text-on-surface-variant hover:bg-surface-container-high hover:text-on-surface cursor-pointer focus-visible:outline focus-visible:outline-2 focus-visible:outline-primary"
          >
            <span className="material-symbols-outlined icon-md" aria-hidden="true">more_horiz</span>
          </button>
          {menuOpen && (
            <DropdownMenu
              open
              onClose={() => setMenuOpen(false)}
              items={menuItems}
              align="end"
              className="w-36 min-w-0"
              ariaLabel={t('memory.card.menu.aria')}
            />
          )}
        </span>
      </div>
    </div>
  )
}
