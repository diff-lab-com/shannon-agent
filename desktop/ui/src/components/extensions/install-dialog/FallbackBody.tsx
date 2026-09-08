// InstallDialog fallback body — used when the entry is `native`/`custom` or
// when a `featured_vendor` lacks `oauth_remote` metadata. Tells the user to
// configure the extension in its dedicated tab and provides a button that
// navigates via `KIND_ROUTE`.
//
// Extracted from InstallDialog.tsx (T3.1).

import { FormattedMessage } from 'react-intl'
import { Button } from '@/components/ui/button'
import { KIND_ROUTE } from './types'
import type { AddonKind } from '@/types'

interface FallbackBodyProps {
  kind: AddonKind
  onOpenTab: () => void
}

export function FallbackBody({ kind, onOpenTab }: FallbackBodyProps) {
  const route = KIND_ROUTE[kind]
  return (
    <div className="flex flex-col gap-md">
      <p className="text-label-sm text-on-surface-variant">
        <FormattedMessage id="extensions.installDialog.manualHint" />
      </p>
      {route ? (
        <Button
          type="button"
          onClick={onOpenTab}
          className="px-md py-sm rounded-lg hover:bg-primary/90 cursor-pointer"
        >
          <span className="material-symbols-outlined icon-sm">tab</span>
          <FormattedMessage id="extensions.installDialog.openTab" />
        </Button>
      ) : null}
    </div>
  )
}