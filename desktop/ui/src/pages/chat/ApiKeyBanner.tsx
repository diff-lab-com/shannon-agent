import { Button } from '@/components/ui/button'
import { Banner } from '@/components/ui/banner'
import { useT } from '@/i18n'

/// `no-provider`: nothing configured at all (no active provider, no env
/// fallback). `no-key`: an active provider exists but its credential store
/// entry is missing — copy names the provider so the fix is unambiguous
/// (2026-09-29 provider review §3-A1: accurate copy for both cases).
export type ApiKeyBannerVariant = 'no-provider' | 'no-key'

interface ApiKeyBannerProps {
  visible: boolean
  variant?: ApiKeyBannerVariant
  /** Active provider display name (variant `no-key`). */
  providerName?: string
  onDismiss: () => void
  onOpenSettings: () => void
}

export default function ApiKeyBanner({
  visible,
  variant = 'no-provider',
  providerName,
  onDismiss,
  onOpenSettings,
}: ApiKeyBannerProps) {
  const t = useT()
  if (!visible) return null
  const title = variant === 'no-key'
    ? t('chat.banner.providerKeyMissing.title', { provider: providerName ?? '' })
    : t('chat.banner.apiKeyMissing.title')
  const body = variant === 'no-key'
    ? t('chat.banner.providerKeyMissing.body', { provider: providerName ?? '' })
    : t('chat.banner.apiKeyMissing.body')
  return (
    <Banner
      tone="info"
      className="shannon-apikey-banner"
      onDismiss={onDismiss}
      dismissLabel={t('chat.banner.apiKeyMissing.dismiss')}
    >
      <span className="material-symbols-outlined text-secondary icon-md shrink-0 mt-[2px]">key_alert</span>
      <div className="flex-1 min-w-0">
        <p className="font-label-md text-on-surface">{title}</p>
        <p className="font-body-sm text-on-surface-variant mt-xs">{body}</p>
      </div>
      <Button
        type="button"
        onClick={onOpenSettings}
        className="shannon-apikey-banner-cta shrink-0 px-md py-xs bg-primary text-on-primary rounded-lg font-label-md cursor-pointer hover:bg-primary/90 focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-primary"
      >
        {t('chat.banner.apiKeyMissing.cta')}
      </Button>
    </Banner>
  )
}
