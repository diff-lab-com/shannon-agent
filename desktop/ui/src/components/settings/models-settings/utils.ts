// Toast helper shared by ModelsSettings sub-components. Lives in utils (not
// types) because it pulls in `sonner` for side-effecting toasts.

import { toast } from 'sonner'
import type { useIntl } from 'react-intl'
import * as api from '@/lib/tauri-api'

export function toastTestResult(
  intl: ReturnType<typeof useIntl>,
  result: api.TestConnectionResult,
  provider: string,
): void {
  const t = (id: string) => intl.formatMessage({ id })
  switch (result.kind) {
    case 'success':
      toast.success(t('settings.models.testResult.success'))
      return
    case 'invalid_key':
      toast.error(t('settings.models.testResult.invalidKey'))
      return
    case 'rate_limited':
      toast.warning(t('settings.models.testResult.rateLimited'))
      return
    case 'provider_error':
      toast.error(intl.formatMessage({ id: 'settings.models.testResult.providerError' }, { provider, status: result.status }))
      return
    case 'network_unreachable':
      toast.error(intl.formatMessage({ id: 'settings.models.testResult.networkUnreachable' }, { provider }))
      return
    case 'unknown':
      toast.error(intl.formatMessage({ id: 'settings.models.testResult.unknown' }, { message: result.message }))
      return
  }
}