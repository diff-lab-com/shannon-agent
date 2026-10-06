import { useIntl } from 'react-intl'
import { cn } from '@/lib/utils'

/**
 * When a setting takes effect — the shared semantics badge for the
 * settings-parity R3 cards. Four kinds (brief T1):
 *
 * - `instant`         — applies immediately (neutral outline)
 * - `new-session`     — applies to sessions started afterwards (secondary)
 * - `restart-app`     — needs an app restart (warning/amber tone)
 * - `restart-gateway` — needs a gateway restart (tertiary tone)
 *
 * Replaces the free-text 「重启后生效」 note lines so the effect semantics
 * stay visually consistent across the Network/Session/Permissions cards.
 */
export type EffectKind = 'instant' | 'new-session' | 'restart-app' | 'restart-gateway'

const KIND_TEXT_ID: Record<EffectKind, string> = {
  instant: 'settings.effect.instant',
  'new-session': 'settings.effect.newSession',
  'restart-app': 'settings.effect.restartApp',
  'restart-gateway': 'settings.effect.restartGateway',
}

const KIND_CLASS: Record<EffectKind, string> = {
  instant: 'border border-outline-variant/50 text-on-surface-variant',
  'new-session': 'bg-secondary-container text-on-secondary-container',
  'restart-app': 'bg-warning-container text-on-warning-container',
  'restart-gateway': 'bg-tertiary-container text-on-tertiary-container',
}

export default function EffectBadge({ kind, className }: { kind: EffectKind; className?: string }) {
  const intl = useIntl()
  return (
    <span
      className={cn(
        'inline-flex items-center px-sm py-[2px] rounded-full text-label-xs font-bold whitespace-nowrap',
        KIND_CLASS[kind],
        className,
      )}
    >
      {intl.formatMessage({ id: KIND_TEXT_ID[kind] })}
    </span>
  )
}
