// S3-1 (P-N11) — the ONE model-picker row renderer, shared by the composer
// chip (ChatInput) and the Header selector.
//
// The review's P-N11 finding: the same catalog rendered at two different
// information densities (chip rows had context+price+vision; Header rows
// only context), and the S2-A `source` wire field was only rendered in the
// Settings catalog. Both surfaces now compose THIS row so a model reads the
// same everywhere: name · capability dots · source badge · why-active label
// · context/price meta. The Settings catalog keeps its richer card layout
// but reuses the source badge below for identical styling.
//
// Badges carry data-testids mirroring the Settings catalog's
// (`source-badge-declared`), so tests can anchor provenance on any surface.
//
// i18n: the source badge reuses the Settings catalog's keys
// (`settings.models.sourceBadge.*`) — same concept, same words. The
// why-active labels live under the two picker namespaces this batch owns
// (session/tier under chat.input.model.why.*, profile/global under
// header.model.why.*) so each surface's namespace stays self-describing.

import { useIntl } from 'react-intl'
import type { ModelInfo } from '@/types'
import type { ModelWhy } from '@/lib/modelWhy'
import { modelPickerMeta } from '@/components/settings/models-settings/types'
import { cn } from '@/lib/utils'

const BADGE_BASE = 'px-xs py-[2px] rounded-sm text-label-2xs font-bold shrink-0'

/**
 * Provenance badge (裁定③/S2-1): `declared` = the provider's curated vault
 * (「模型仓」), `overlay` = the models.dev refresh. Catalog rows — the
 * default — stay unbadged, exactly like the Settings catalog list.
 */
export function ModelSourceBadge({ source }: { source: ModelInfo['source'] }) {
  const { formatMessage } = useIntl()
  if (source !== 'overlay' && source !== 'declared') return null
  const key =
    source === 'overlay'
      ? 'settings.models.sourceBadge.overlay'
      : 'settings.models.sourceBadge.declared'
  return (
    <span
      data-testid={`source-badge-${source}`}
      title={formatMessage({ id: key })}
      className={cn(BADGE_BASE, 'bg-secondary-container text-on-secondary-container')}
    >
      {formatMessage({ id: key })}
    </span>
  )
}

/**
 * Why-active label (Claude Code picker-label paradigm): tells the user WHY
 * this row is the effective model — session override > plan/act tier >
 * profile-pinned / plain global default (engine precedence, mirrored in
 * `lib/modelWhy.ts`). `null` why → nothing renders.
 */
export function ModelWhyBadge({ why }: { why: ModelWhy | null }) {
  const { formatMessage } = useIntl()
  if (!why) return null
  let label: string
  let tone: string
  switch (why.kind) {
    case 'session':
      label = formatMessage({ id: 'chat.input.model.why.session' })
      // Strongest treatment — a live session pin is the top of the chain.
      tone = 'bg-primary text-on-primary'
      break
    case 'tier':
      label = formatMessage(
        { id: 'chat.input.model.why.tier' },
        {
          phase: formatMessage({
            id: why.phase === 'plan' ? 'chat.phaseTier.planShort' : 'chat.phaseTier.actShort',
          }),
        },
      )
      tone = 'bg-primary-container text-on-primary-container'
      break
    case 'profile':
      label = formatMessage(
        { id: 'header.model.why.profile' },
        { name: why.profile },
      )
      tone = 'bg-surface-container-high text-on-surface-variant'
      break
    case 'global':
      label = formatMessage({ id: 'header.model.why.global' })
      tone = 'bg-surface-container-high text-on-surface-variant'
      break
  }
  return (
    <span
      data-testid={`why-badge-${why.kind}`}
      title={label}
      className={cn(BADGE_BASE, tone)}
    >
      {label}
    </span>
  )
}

/**
 * The shared row content: name + capability dots + badges, meta right.
 * `compact` drops the tools icon (the Header menu's 340px popup keeps the
 * vision dot + badges + meta; the chip popup is wider and shows both).
 * Both surfaces pass the same model, so the parity pin is structural, not
 * a copy.
 */
export function ModelPickerRowContent({
  model,
  why,
  compact = false,
}: {
  model: ModelInfo
  why: ModelWhy | null
  /** Header menu rows: skip the tools icon, keep everything else. */
  compact?: boolean
}) {
  const { formatMessage } = useIntl()
  return (
    <span className="flex w-full min-w-0 items-center gap-xs">
      <span className="font-mono truncate shrink-0">{model.name}</span>
      {/* R2-3: vision dot — real catalog metadata only; unknown renders
          nothing (never guessed). */}
      {model.vision === true && (
        <span
          aria-label={formatMessage({ id: 'chat.input.model.vision' })}
          title={formatMessage({ id: 'chat.input.model.vision' })}
          className="inline-block size-1.5 shrink-0 rounded-full bg-primary"
        />
      )}
      {/* S2-4b tool bit — three-state like vision: `true` shows the mark,
          unknown/false render nothing. Compact (Header) rows skip it. */}
      {!compact && model.tools === true && (
        <span
          aria-label={formatMessage({ id: 'settings.models.toolsBadge' })}
          title={formatMessage({ id: 'settings.models.toolsBadge' })}
          className="material-symbols-outlined icon-sm text-on-surface-variant shrink-0"
        >
          build
        </span>
      )}
      <ModelSourceBadge source={model.source} />
      <ModelWhyBadge why={why} />
      <span className="ml-auto shrink-0 whitespace-nowrap font-label-xs text-on-surface-variant tabular-nums">
        {modelPickerMeta(model)}
      </span>
    </span>
  )
}
