import { Button } from '@/components/ui/button'
import { Banner } from '@/components/ui/banner'

interface ApiKeyBannerProps {
  t: (id: string) => string
  visible: boolean
  onDismiss: () => void
  onOpenSettings: () => void
}

export default function ApiKeyBanner({ t, visible, onDismiss, onOpenSettings }: ApiKeyBannerProps) {
  if (!visible) return null
  return (
    <Banner
      tone="info"
      className="shannon-apikey-banner"
      onDismiss={onDismiss}
      dismissLabel={t('chat.banner.apiKeyMissing.dismiss')}
    >
      <span className="material-symbols-outlined text-secondary icon-md shrink-0 mt-[2px]">key_alert</span>
      <div className="flex-1 min-w-0">
        <p className="font-label-md text-on-surface">{t('chat.banner.apiKeyMissing.title')}</p>
        <p className="font-body-sm text-on-surface-variant mt-xs">{t('chat.banner.apiKeyMissing.body')}</p>
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