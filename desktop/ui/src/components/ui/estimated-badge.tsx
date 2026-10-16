import { useT } from '@/i18n'

/** G3 (2026-10-15 parity review §2.3, 裁决 B 中道档): the one「估算」
 *  annotation every surviving USD display carries. Amber container pair
 *  (the semantic warning tones — never an accent-tint composite, the
 *  design-token gate rejects those), mini outline chip geometry so it can
 *  ride next to a $ figure without becoming a second headline.
 *
 *  Unified mechanism ruling (task §2): badge, not tooltip — one visible,
 *  locale-translated marker everywhere a cost is an estimate. Budget-control
 *  faces (caps, budget inputs) are configured USD and deliberately NOT
 *  badged; only derived/estimated spend is. */
export default function EstimatedBadge({ className }: { className?: string }) {
  const t = useT()
  return (
    <span
      data-testid="estimated-badge"
      title={t('usage.estimated.title')}
      className={
        'inline-flex items-center rounded-full border border-warning/30 bg-warning-container text-on-warning-container px-1.5 py-px text-label-2xs font-bold uppercase tracking-wider leading-none shrink-0'
        + (className ? ` ${className}` : '')
      }
    >
      {t('usage.estimated.badge')}
    </span>
  )
}
