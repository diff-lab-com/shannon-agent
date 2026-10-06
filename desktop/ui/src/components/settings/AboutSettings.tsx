import { useEffect, useState } from 'react'
import { getVersion } from '@tauri-apps/api/app'
import { useIntl } from 'react-intl'
import { toast } from 'sonner'
import { Button } from '@/components/ui/button'
import { Spinner } from '@/components/ui/loading-state'
import * as api from '@/lib/tauri-api'
import { toastError } from '@/lib/errorToast'
import type { AppUpdateInfo } from '@/lib/tauri-api'
import { cn } from '@/lib/utils'

/**
 * Settings → 关于 (Settings R3, T1). The version / check-update / release
 * page content migrates out of the dev-gated Advanced section so normal
 * users can reach it, plus a new read-only data-directory line.
 */
export default function AboutSettings() {
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })

  // Real app version (same source Advanced's dev card already used).
  const [appVersion, setAppVersion] = useState<string | null>(null)
  // C1① — semi-automatic update check (GitHub latest → open release page).
  const [updateInfo, setUpdateInfo] = useState<AppUpdateInfo | null>(null)
  const [checkingUpdate, setCheckingUpdate] = useState(false)
  // Shannon data directory, read-only (moved here is a manual migration).
  const [shannonHome, setShannonHome] = useState<string | null>(null)

  useEffect(() => {
    getVersion().then(setAppVersion).catch(() => { /* jsdom / denied ACL — show no version */ })
    api.getShannonHome().then(setShannonHome).catch(() => { /* backend missing — show no path */ })
  }, [])

  const handleCheckUpdate = async () => {
    setCheckingUpdate(true)
    try {
      const info = await api.checkAppUpdate()
      setUpdateInfo(info)
      if (info.updateAvailable && info.latestVersion) {
        toast.success(intl.formatMessage({ id: 'settings.about.updateAvailable' }, { version: info.latestVersion }))
      }
    } catch (e) {
      toastError(t('settings.advanced.updateCheckFailed'), e)
    }
    setCheckingUpdate(false)
  }

  const handleOpenReleasePage = async () => {
    if (!updateInfo) return
    try {
      await api.openReleasePage(updateInfo.releaseUrl)
    } catch (e) {
      toastError(t('settings.advanced.updateOpenFailed'), e)
    }
  }

  return (
    <div className="pb-xl">
      <p className="text-on-surface-variant font-body-md mb-xl">{t('settings.about.help')}</p>

      <div className="grid grid-cols-1 gap-gutter">
        {/* About — version, update check, release page */}
        <div className="bg-surface-container-lowest p-xl rounded-xl shadow-e1 border border-outline-variant/30 group hover:shadow-e2 transition-shadow" data-testid="about-card">
          <div className="flex items-center gap-md mb-md">
            <div className="p-sm bg-primary-container rounded-lg text-on-primary-container flex items-center justify-center">
              <span className="material-symbols-outlined">info</span>
            </div>
            <h3 className="font-headline-md text-headline-md font-bold text-on-surface">{t('settings.about.title')}</h3>
            {updateInfo && (
              <span
                className={cn(
                  'ml-auto px-sm py-[2px] rounded-full text-label-xs font-bold whitespace-nowrap',
                  updateInfo.updateAvailable
                    ? 'bg-tertiary-container text-on-tertiary-container'
                    : 'bg-surface-container-high text-on-surface-variant',
                )}
                data-testid="about-update-badge"
              >
                {updateInfo.error
                  ? t('settings.advanced.updateCheckFailed')
                  : updateInfo.updateAvailable && updateInfo.latestVersion
                    ? intl.formatMessage({ id: 'settings.about.updateAvailable' }, { version: updateInfo.latestVersion })
                    : t('settings.about.latest')}
              </span>
            )}
          </div>
          <div className="flex flex-col md:flex-row md:items-center justify-between gap-lg">
            <div className="flex-1">
              <p className="text-on-surface-variant text-body-sm mb-md flex items-center gap-sm">
                {t('settings.about.version')}
                {appVersion && (
                  <span className="px-sm py-[2px] rounded-full bg-surface-container-high text-on-surface-variant text-label-xs font-bold whitespace-nowrap">
                    v{appVersion}
                  </span>
                )}
              </p>
              {updateInfo?.error && (
                <p className="text-error text-label-sm mt-xs">{updateInfo.error}</p>
              )}
            </div>
            <div className="flex items-center gap-md shrink-0">
              {updateInfo && !updateInfo.error && (
                <Button
                  variant="ghost"
                  className="flex items-center gap-xs text-link font-label-md text-body-sm hover:underline cursor-pointer"
                  onClick={handleOpenReleasePage}
                >
                  <span className="material-symbols-outlined icon-sm">open_in_new</span>
                  {t('settings.about.openRelease')}
                </Button>
              )}
              <Button
                className="px-xl py-md bg-primary text-on-primary rounded-xl font-label-md text-body-sm font-bold hover:bg-primary/90 shadow-e2 active:scale-[0.98] transition-all whitespace-nowrap cursor-pointer"
                onClick={handleCheckUpdate}
                disabled={checkingUpdate}
              >
                {checkingUpdate ? (
                  <>
                    <Spinner className="mr-sm text-body-lg" />
                    {t('settings.about.checking')}
                  </>
                ) : (
                  t('settings.about.checkUpdate')
                )}
              </Button>
            </div>
          </div>
        </div>

        {/* Data directory — read-only; $SHANNON_HOME overrides it, moving it
            is a manual migration. The log-directory entry stays in Advanced. */}
        <div className="bg-surface-container-lowest p-xl rounded-xl shadow-e1 border border-outline-variant/30 group hover:shadow-e2 transition-shadow" data-testid="about-datadir-card">
          <div className="flex items-center gap-md mb-md">
            <div className="p-sm bg-secondary-container rounded-lg text-on-secondary-container flex items-center justify-center">
              <span className="material-symbols-outlined">folder_open</span>
            </div>
            <h3 className="font-headline-md text-headline-md font-bold text-on-surface">{t('settings.about.dataDir')}</h3>
          </div>
          <p className="text-on-surface-variant text-body-sm mb-md">{t('settings.about.dataDirHelp')}</p>
          {shannonHome && (
            <code
              className="block px-md py-sm bg-surface-container-low rounded-lg border border-outline-variant/30 text-body-sm font-mono text-on-surface break-all"
              data-testid="about-datadir-path"
            >
              {shannonHome}
            </code>
          )}
        </div>
      </div>
    </div>
  )
}
