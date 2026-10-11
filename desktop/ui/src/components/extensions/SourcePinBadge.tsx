import { useIntl } from 'react-intl'

import { cn } from '@/lib/utils'

/**
 * Source-pin chip for repo-installed extensions (扩展安全专项·步骤 B).
 *
 * - `refFloating` → warning chip: the install tracks a mutable ref, so a
 *   refresh can silently swap the content upstream. This is the honest
 *   supply-chain signal; pinned installs render nothing here.
 * - `sourceCommit` → short SHA in mono so the user can eyeball what the
 *   content is supposed to be.
 */
export function SourcePinBadge({
  sourceCommit,
  refFloating,
  className,
}: {
  sourceCommit: string | null
  refFloating: boolean
  className?: string
}) {
  const intl = useIntl()
  if (!refFloating && !sourceCommit) return null
  return (
    <span
      className={cn('inline-flex items-center gap-[4px] shrink-0', className)}
      data-testid="source-pin"
    >
      {refFloating && (
        <span
          className="inline-flex items-center gap-[2px] text-label-xs px-xs py-[1px] rounded-full font-bold bg-error-container/40 text-on-error-container"
          title={intl.formatMessage({ id: 'extensions.source.floatingTip' })}
          data-testid="source-pin-floating"
        >
          <span className="material-symbols-outlined icon-sm">link_off</span>
          {intl.formatMessage({ id: 'extensions.source.floating' })}
        </span>
      )}
      {sourceCommit && (
        <span
          className="text-label-xs text-on-surface-variant font-mono"
          title={intl.formatMessage({ id: 'extensions.source.commitTip' })}
        >
          @{sourceCommit.slice(0, 7)}
        </span>
      )}
    </span>
  )
}
