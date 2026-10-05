// ToolGroupCard — Settings R3 T11 (C6): the foldable wrapper around a run of
// consecutive same-kind tool cards (Explore / Terminal / Changes).
//
// Presentation mirrors the SubagentBlock collapse pattern in MessageBubble
// (full-width header button + expand_more/expand_less affordance, default
// collapsed, children render in place when opened). The grouping decision —
// WHICH cards land here — lives in lib/toolGrouping; this component is the
// shell only. Streaming keeps per-card rendering by ruling R10.

import { useState } from 'react'
import { useIntl } from 'react-intl'
import { cn } from '@/lib/utils'
import type { ToolGroupKind } from '@/lib/toolGrouping'

const KIND_ICONS: Record<ToolGroupKind, string> = {
  explore: 'travel_explore',
  terminal: 'terminal',
  changes: 'edit_note',
}

const KIND_TITLES: Record<ToolGroupKind, string> = {
  explore: 'chat.toolGroup.exploreTitle',
  terminal: 'chat.toolGroup.terminalTitle',
  changes: 'chat.toolGroup.changesTitle',
}

const KIND_ACCENTS: Record<ToolGroupKind, string> = {
  explore: 'text-secondary',
  terminal: 'text-primary',
  changes: 'text-tertiary',
}

export interface ToolGroupCardProps {
  kind: ToolGroupKind
  /** Number of tool cards folded into this group (the header badge). */
  count: number
  /** First tool name in the run — the header summary's head. */
  firstToolName?: string
  /** Last tool name in the run — shown after an ellipsis when it differs
   *  from the first, so a heterogeneous run stays identifiable collapsed. */
  lastToolName?: string
  /** The original ToolCallDisplay cards, rendered in place when expanded. */
  children: React.ReactNode
}

export function ToolGroupCard({ kind, count, firstToolName, lastToolName, children }: ToolGroupCardProps) {
  const intl = useIntl()
  const [expanded, setExpanded] = useState(false)
  const showLast = count > 1 && lastToolName != null && lastToolName !== firstToolName

  return (
    <div className="rounded-xl border border-outline-variant/30 bg-surface-container-lowest/60 overflow-hidden" data-testid="tool-group-card" data-group-kind={kind}>
      <button
        type="button"
        onClick={() => setExpanded(!expanded)}
        aria-expanded={expanded}
        className="w-full flex items-center gap-sm px-sm py-xs text-left cursor-pointer hover:bg-surface-container transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary/30"
        data-testid="tool-group-header"
        aria-label={intl.formatMessage({ id: 'chat.toolGroup.toggleAria' }, { count })}
      >
        <span className={cn('material-symbols-outlined icon-sm shrink-0', KIND_ACCENTS[kind])} aria-hidden="true">
          {KIND_ICONS[kind]}
        </span>
        <span className="font-label-md text-on-surface shrink-0">{intl.formatMessage({ id: KIND_TITLES[kind] })}</span>
        <span
          className="font-mono text-label-xs tabular-nums text-on-surface-variant px-xs py-[1px] rounded-sm bg-surface-container shrink-0"
          data-testid="tool-group-count"
        >
          {intl.formatMessage({ id: 'chat.toolGroup.count' }, { count })}
        </span>
        {firstToolName != null && (
          <span className="font-mono text-label-xs text-on-surface-variant truncate flex-1" data-testid="tool-group-summary">
            {firstToolName}
            {showLast && <span aria-hidden="true"> … {lastToolName}</span>}
          </span>
        )}
        <span className="material-symbols-outlined icon-sm text-on-surface-variant shrink-0" aria-hidden="true">
          {expanded ? 'expand_less' : 'expand_more'}
        </span>
      </button>
      {expanded && (
        <div className="px-sm pb-sm space-y-sm" data-testid="tool-group-body">
          {children}
        </div>
      )}
    </div>
  )
}

export default ToolGroupCard
