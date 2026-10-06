// Toast + categorized-message helpers shared by ModelsSettings sub-components
// and the Add/Edit Provider modal. Lives in utils (not types) because it pulls
// in `sonner` for side-effecting toasts.

import { toast } from 'sonner'
import type { useIntl } from 'react-intl'
import type * as api from '@/lib/tauri-api'

/// Categorized, localized text for a [`api.TestConnectionResult`] — the
/// inline (non-toast) counterpart to `toastTestResult`. Shared by the
/// in-modal "Test connection" status line so the modal and the saved-card
/// tests read identically (review §3-B item 11).
export function testResultMessage(
  intl: ReturnType<typeof useIntl>,
  result: api.TestConnectionResult,
  provider: string,
): string {
  const t = (id: string) => intl.formatMessage({ id })
  switch (result.kind) {
    case 'success':
      return t('settings.models.testResult.success')
    case 'invalid_key':
      return t('settings.models.testResult.invalidKey')
    case 'rate_limited':
      return t('settings.models.testResult.rateLimited')
    // R2-P1-10: HTTP 402 — quota/billing exhausted, not a bad key.
    case 'quota_exhausted':
      return t('settings.models.testResult.quotaExhausted')
    case 'provider_error':
      return intl.formatMessage({ id: 'settings.models.testResult.providerError' }, { provider, status: result.status })
    case 'network_unreachable':
      return intl.formatMessage({ id: 'settings.models.testResult.networkUnreachable' }, { provider })
    case 'unknown':
      return intl.formatMessage({ id: 'settings.models.testResult.unknown' }, { message: result.message })
  }
}

/// Localized inline text for a failed `fetchProviderModels` call — maps the
/// backend's categorized error tokens (see `parseFetchModelsError`) to the
/// same message family the connection tests use.
export function fetchFailureMessage(
  intl: ReturnType<typeof useIntl>,
  failure: api.FetchModelsFailure,
  provider: string,
): string {
  switch (failure.kind) {
    case 'invalid_key':
      return intl.formatMessage({ id: 'settings.models.testResult.invalidKey' })
    case 'rate_limited':
      return intl.formatMessage({ id: 'settings.models.testResult.rateLimited' })
    case 'network_unreachable':
      return intl.formatMessage({ id: 'settings.models.testResult.networkUnreachable' }, { provider })
    case 'provider_error':
      return intl.formatMessage({ id: 'settings.models.testResult.providerError' }, { provider, status: failure.status })
    case 'unsupported_kind':
      return intl.formatMessage({ id: 'settings.models.fetchError.unsupportedKind' })
    case 'missing_key':
      return intl.formatMessage({ id: 'settings.models.fetchError.missingKey' })
    case 'invalid_base_url':
      return intl.formatMessage({ id: 'settings.models.fetchError.invalidBaseUrl' }, { detail: failure.detail })
    case 'unknown':
      return intl.formatMessage({ id: 'settings.models.testResult.unknown' }, { message: failure.message })
  }
}

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
    // R2-P1-10: HTTP 402 — quota/billing exhausted, not a bad key.
    case 'quota_exhausted':
      toast.error(t('settings.models.testResult.quotaExhausted'))
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