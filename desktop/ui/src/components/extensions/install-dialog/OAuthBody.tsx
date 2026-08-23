// InstallDialog body for `featured_vendor` + `metadata.transport=oauth_remote`.
// Renders vendor info, endpoint input (read-only), scope pills, and a
// "Connect" button. The backend OAuth installer is not wired yet, so the
// button only fires a toast.
//
// Extracted from InstallDialog.tsx (T3.1).

import { FormattedMessage, useIntl } from 'react-intl'
import { Button } from '@/components/ui/button'

interface OAuthBodyProps {
  vendor: string
  endpoint: string | undefined
  scopes: string[]
  onConnect: () => void
}

export function OAuthBody({ vendor, endpoint, scopes, onConnect }: OAuthBodyProps) {
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })

  return (
    <div className="flex flex-col gap-md">
      <p className="text-label-sm text-on-surface-variant">{vendor}</p>
      <label className="flex flex-col gap-xs">
        <span className="text-label-sm font-bold text-on-surface">
          <FormattedMessage id="extensions.installDialog.endpoint" />
        </span>
        <input
          readOnly
          value={endpoint ?? ''}
          aria-label={t('extensions.installDialog.endpoint')}
          className="bg-surface-container-low border border-outline-variant/40 rounded-lg px-md py-sm font-body-md font-mono text-on-surface text-label-sm"
        />
      </label>
      <label className="flex flex-col gap-xs">
        <span className="text-label-sm font-bold text-on-surface">
          <FormattedMessage id="extensions.installDialog.scopes" />
        </span>
        <div className="flex flex-wrap gap-xs">
          {scopes.length === 0 ? (
            <span className="text-label-sm text-on-surface-variant">—</span>
          ) : (
            scopes.map((s) => (
              <span
                key={s}
                className="px-xs py-[2px] rounded-full bg-surface-container-high text-label-xs text-on-surface-variant font-mono"
              >
                {s}
              </span>
            ))
          )}
        </div>
      </label>
      <Button
        type="button"
        onClick={onConnect}
        className="px-md py-sm rounded-lg hover:bg-primary/90 disabled:opacity-60 cursor-pointer"
      >
        <span className="material-symbols-outlined icon-sm">link</span>
        <FormattedMessage id="extensions.installDialog.connect" />
      </Button>
    </div>
  )
}