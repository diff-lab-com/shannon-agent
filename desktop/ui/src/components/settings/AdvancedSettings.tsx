import { useState, useEffect } from 'react'
import { Spinner } from '@/components/ui/loading-state'
import { useIntl } from 'react-intl'
import { toast } from 'sonner'
import { Button } from '@/components/ui/button'
import { Switch } from '@/components/ui/switch'
import { Modal } from '@/components/ui/modal'
import { ConfirmDialog } from '@/components/ui/confirm-dialog'
import { useCatalog } from '@/context/CatalogContext'
import { SkillApprovalModal } from '@/components/self-improve/SkillApprovalModal'
import { VoiceSttSettings } from '@/components/settings/VoiceSttSettings'
import { VoiceLocalSettings } from '@/components/settings/VoiceLocalSettings'
import * as api from '@/lib/tauri-api'
import { toastError } from '@/lib/errorToast'
import type { SkillCandidate, CliInstallStatus, AppUpdateInfo } from '@/lib/tauri-api'
import { cn } from '@/lib/utils'

export default function AdvancedSettings() {
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })
  const { refreshConfig, config } = useCatalog()
  const [memoryEnabled, setMemoryEnabled] = useState(config?.memory_enabled ?? true)
  const [telemetryEnabled, setTelemetryEnabled] = useState(config?.telemetry_enabled ?? false)
  const [encryptionEnabled, setEncryptionEnabled] = useState(config?.encryption_enabled ?? true)
  const [debugConsole, setDebugConsole] = useState(config?.debug_console ?? false)
  const [skillLoopEnabled, setSkillLoopEnabled] = useState(config?.skill_loop_enabled ?? false)
  const [skillDetectionEnabled, setSkillDetectionEnabled] = useState(config?.skill_detection_enabled ?? true)
  const [clearing, setClearing] = useState(false)
  const [resetting, setResetting] = useState(false)
  const [showLogs, setShowLogs] = useState(false)
  const [showApiKeys, setShowApiKeys] = useState(false)
  const [showResetConfirm, setShowResetConfirm] = useState(false)
  const [showClearConfirm, setShowClearConfirm] = useState(false)

  const [candidates, setCandidates] = useState<SkillCandidate[]>([])
  const [candidateIndex, setCandidateIndex] = useState(0)
  const [approvalOpen, setApprovalOpen] = useState(false)

  // ADR-0011 B3 — bundled `shannon` CLI exposure (non-shadowing install).
  const [cliStatus, setCliStatus] = useState<CliInstallStatus | null>(null)
  const [installingCli, setInstallingCli] = useState(false)

  // C1① — semi-automatic update check (GitHub latest → open download page).
  const [updateInfo, setUpdateInfo] = useState<AppUpdateInfo | null>(null)
  const [checkingUpdate, setCheckingUpdate] = useState(false)

  useEffect(() => {
    let cancelled = false
    listSkillCandidatesSafe(!cancelled)
    api.getCliInstallStatus()
      .then((s) => { if (!cancelled) setCliStatus(s) })
      .catch(() => { if (!cancelled) setCliStatus(null) })
    return () => { cancelled = true }

    function listSkillCandidatesSafe(active: boolean) {
      api.listSkillCandidates()
        .then((rows) => { if (active) setCandidates(rows) })
        .catch(() => { if (active) setCandidates([]) })
    }
  }, [])

  const handleToggle = async (key: string, value: boolean, setter: (v: boolean) => void) => {
    setter(value)
    try {
      await api.configure({ key, value: String(value) })
      await refreshConfig()
      toast.success(intl.formatMessage({ id: 'settings.advanced.toggled' }, { key: key.replace(/_/g, ' '), state: value ? t('settings.advanced.enabled') : t('settings.advanced.disabled') }))
    } catch (e) { toastError(t('settings.advanced.updateFailed'), e) }
  }

  const handleClearCache = async () => {
    setClearing(true)
    try { await api.configure({ key: 'clear_cache', value: 'true' }); toast.success(t('settings.advanced.cacheCleared')) } catch (e) { toastError(t('settings.advanced.clearCacheFailed'), e) }
    setClearing(false)
  }

  const handleFactoryReset = async () => {
    setResetting(true)
    try { await api.configure({ key: 'factory_reset', value: 'true' }); toast.success(t('settings.advanced.resetComplete')) } catch (e) { toastError(t('settings.advanced.resetFailed'), e) }
    setResetting(false)
    setShowResetConfirm(false)
  }

  const handleInstallCli = async () => {
    setInstallingCli(true)
    try {
      const result = await api.installCliToPath()
      setCliStatus(result.status)
      if (result.installedLink) {
        toast.success(intl.formatMessage({ id: 'settings.advanced.cliInstalledToast' }, { link: result.installedLink }))
      } else {
        toast.message(result.message)
      }
    } catch (e) {
      toastError(t('settings.advanced.cliInstallFailed'), e)
    }
    setInstallingCli(false)
  }

  const handleCheckUpdate = async () => {
    setCheckingUpdate(true)
    try {
      const info = await api.checkAppUpdate()
      setUpdateInfo(info)
      if (info.updateAvailable && info.latestVersion) {
        toast.success(intl.formatMessage({ id: 'settings.advanced.updateAvailableBadge' }, { version: info.latestVersion }))
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

  function advanceCandidate() {
    setCandidates((prev) => {
      const next = prev.slice(1)
      if (next.length === 0) setApprovalOpen(false)
      return next
    })
  }

  return (
    <div className="pb-xl">
      <div className="mb-xl">
        <h2 className="font-headline-lg text-headline-lg text-on-surface mb-sm">{t('settings.advanced.title')}</h2>
        <p className="text-on-surface-variant font-body-md">{t('settings.advanced.subtitle')}</p>
      </div>

      <div className="grid grid-cols-1 lg:grid-cols-2 gap-gutter">
        {/* Skill Extraction */}
        <div className="bg-surface-container-lowest p-lg rounded-xl shadow-sm border border-outline-variant/30 group hover:shadow-md transition-shadow">
          <div className="flex items-center gap-md mb-md">
            <div className="p-2 bg-tertiary/10 rounded-lg text-tertiary flex items-center justify-center">
              <span className="material-symbols-outlined">auto_awesome</span>
            </div>
            <h3 className="font-headline-md text-[24px] font-bold text-on-surface">{t('settings.skillLoop.title')}</h3>
            {candidates.length > 0 && (
              <span className="ml-auto px-sm py-[2px] rounded-full bg-tertiary-container text-on-tertiary-container text-label-xs font-bold">
                {intl.formatMessage({ id: 'settings.skillLoop.pendingCount' }, { count: candidates.length })}
              </span>
            )}
          </div>
          <p className="text-on-surface-variant text-body-sm mb-lg">{t('settings.skillLoop.description')}</p>
          <div className="flex items-center justify-between gap-md">
            <div>
              <div className="font-label-md text-[14px] text-on-surface font-semibold mb-1">{t('settings.skillLoop.enabled')}</div>
              <div className="font-label-sm text-[12px] text-on-surface-variant leading-tight">{t('settings.skillLoop.enabledDesc')}</div>
            </div>
            <Switch checked={skillLoopEnabled} onCheckedChange={v => handleToggle('skill_loop_enabled', v, setSkillLoopEnabled)} className="shrink-0" />
          </div>
          <div className="flex items-center justify-between gap-md mt-md">
            <div>
              <div className="font-label-md text-[14px] text-on-surface font-semibold mb-1">{t('settings.skillLoop.detectionEnabled')}</div>
              <div className="font-label-sm text-[12px] text-on-surface-variant leading-tight">{t('settings.skillLoop.detectionEnabledDesc')}</div>
            </div>
            <Switch checked={skillDetectionEnabled} onCheckedChange={v => handleToggle('skill_detection_enabled', v, setSkillDetectionEnabled)} className="shrink-0" />
          </div>
          {candidates.length > 0 && (
            <Button
              variant="ghost"
              className="w-full mt-md py-sm border border-tertiary/30 rounded-lg text-tertiary font-label-md font-bold text-[14px] hover:bg-tertiary-container/30 transition-colors cursor-pointer"
              onClick={() => { setCandidateIndex(0); setApprovalOpen(true) }}
            >
              <span className="material-symbols-outlined icon-sm mr-xs">rate_review</span>
              {t('settings.skillLoop.review')}
            </Button>
          )}
        </div>

        {/* Memory Management */}
        <div className="bg-surface-container-lowest p-lg rounded-xl shadow-sm border border-outline-variant/30 group hover:shadow-md transition-shadow">
          <div className="flex items-center gap-md mb-md">
            <div className="p-2 bg-primary/10 rounded-lg text-primary flex items-center justify-center">
              <span className="material-symbols-outlined">memory</span>
            </div>
            <h3 className="font-headline-md text-[24px] font-bold text-on-surface">{t('settings.advanced.memoryTitle')}</h3>
          </div>
          <p className="text-on-surface-variant text-body-sm mb-lg">{t('settings.advanced.memoryDesc')}</p>
          <div className="space-y-md">
            <div className="flex items-center justify-between py-sm gap-md">
              <div>
                <div className="font-label-md text-[14px] text-on-surface font-semibold mb-1">{t('settings.advanced.longTermMemory')}</div>
                <div className="font-label-sm text-[12px] text-on-surface-variant leading-tight">{t('settings.advanced.longTermMemoryDesc')}</div>
              </div>
              <Switch checked={memoryEnabled} onCheckedChange={v => handleToggle('memory_enabled', v, setMemoryEnabled)} className="shrink-0" />
            </div>
            <Button
              className="w-full py-md border border-outline-variant/50 rounded-xl text-on-surface font-label-md font-bold text-[14px] hover:bg-surface-container-low transition-colors active:scale-[0.99] cursor-pointer"
              onClick={() => setShowClearConfirm(true)}
              disabled={clearing}
            >
              {clearing ? <Spinner className="mr-sm text-[18px]" /> : null}
              {clearing ? t('settings.advanced.clearing') : t('settings.advanced.clearSessionCache')}
            </Button>
          </div>
        </div>

        {/* Data Privacy */}
        <div className="bg-surface-container-lowest p-lg rounded-xl shadow-sm border border-outline-variant/30 group hover:shadow-md transition-shadow">
          <div className="flex items-center gap-md mb-md">
            <div className="p-2 bg-secondary/10 rounded-lg text-secondary flex items-center justify-center">
              <span className="material-symbols-outlined" style={{fontVariationSettings: "'FILL' 1"}}>security</span>
            </div>
            <h3 className="font-headline-md text-[24px] font-bold text-on-surface">{t('settings.advanced.dataPrivacy')}</h3>
          </div>
          <p className="text-on-surface-variant text-body-sm mb-lg">{t('settings.advanced.dataPrivacyDesc')}</p>
          <div className="space-y-lg mt-sm">
            <div className="flex items-center justify-between gap-md">
              <div>
                <div className="font-label-md text-[14px] text-on-surface font-semibold mb-1">{t('settings.advanced.anonReporting')}</div>
                <div className="font-label-sm text-[12px] text-on-surface-variant leading-tight">{t('settings.advanced.anonReportingDesc')}</div>
              </div>
              <Switch checked={telemetryEnabled} onCheckedChange={v => handleToggle('telemetry', v, setTelemetryEnabled)} className="shrink-0" />
            </div>
            <div className="flex items-center justify-between gap-md">
              <div>
                <div className="font-label-md text-[14px] text-on-surface font-semibold mb-1">{t('settings.advanced.encryption')}</div>
                <div className="font-label-sm text-[12px] text-on-surface-variant leading-tight">{t('settings.advanced.encryptionDesc')}</div>
              </div>
              <Switch checked={encryptionEnabled} onCheckedChange={v => handleToggle('encryption', v, setEncryptionEnabled)} className="shrink-0" />
            </div>
          </div>
        </div>

        {/* Voice / Speech-to-text (D4 cloud STT) */}
        <VoiceSttSettings />

        {/* Voice / Local (P2-5e whisper-rs) — opt-in offline STT */}
        <VoiceLocalSettings />

        {/* Command line — expose the bundled `shannon` CLI (ADR-0011 B3) */}
        <div className="bg-surface-container-lowest p-lg rounded-xl shadow-sm border border-outline-variant/30 lg:col-span-2 group hover:shadow-md transition-shadow">
          <div className="flex items-center gap-md mb-md">
            <div className="p-2 bg-primary/10 rounded-lg text-primary flex items-center justify-center">
              <span className="material-symbols-outlined">terminal</span>
            </div>
            <h3 className="font-headline-md text-[24px] font-bold text-on-surface">{t('settings.advanced.cliTitle')}</h3>
            <span
              className={cn(
                "ml-auto px-sm py-[2px] rounded-full text-label-xs font-bold whitespace-nowrap",
                cliStatus?.onPath
                  ? 'bg-tertiary-container text-on-tertiary-container'
                  : 'bg-error/10 text-error',
              )}
            >
              {cliStatus?.onPath
                ? (cliStatus.onPathVersion ?? t('settings.advanced.cliInstalled'))
                : t('settings.advanced.cliNotOnPath')}
            </span>
          </div>
          <div className="flex flex-col md:flex-row md:items-center justify-between gap-lg">
            <div className="flex-1">
              <p className="text-on-surface-variant text-body-sm mb-md">{t('settings.advanced.cliDesc')}</p>
              {cliStatus?.handledByInstaller && !cliStatus?.onPath && (
                <p className="text-on-surface-variant text-label-sm">{t('settings.advanced.cliInstallerHint')}</p>
              )}
            </div>
            <Button
              className="px-xl py-md bg-primary text-on-primary rounded-xl font-label-md text-[14px] font-bold hover:bg-primary/90 shadow-md active:scale-[0.98] transition-all whitespace-nowrap cursor-pointer"
              onClick={handleInstallCli}
              disabled={installingCli || !cliStatus || cliStatus.onPath}
            >
              {installingCli ? t('settings.advanced.cliInstalling') : t('settings.advanced.cliInstallButton')}
            </Button>
          </div>
        </div>

        {/* Version & updates — semi-automatic update check (C1①) */}
        <div className="bg-surface-container-lowest p-lg rounded-xl shadow-sm border border-outline-variant/30 lg:col-span-2 group hover:shadow-md transition-shadow">
          <div className="flex items-center gap-md mb-md">
            <div className="p-2 bg-primary/10 rounded-lg text-primary flex items-center justify-center">
              <span className="material-symbols-outlined">system_update_alt</span>
            </div>
            <h3 className="font-headline-md text-[24px] font-bold text-on-surface">{t('settings.advanced.updateTitle')}</h3>
            {updateInfo && (
              <span
                className={cn(
                  "ml-auto px-sm py-[2px] rounded-full text-label-xs font-bold whitespace-nowrap",
                  updateInfo.updateAvailable
                    ? 'bg-tertiary-container text-on-tertiary-container'
                    : 'bg-surface-container-high text-on-surface-variant',
                )}
              >
                {updateInfo.error
                  ? t('settings.advanced.updateCheckFailed')
                  : updateInfo.updateAvailable && updateInfo.latestVersion
                    ? intl.formatMessage({ id: 'settings.advanced.updateAvailableBadge' }, { version: updateInfo.latestVersion })
                    : t('settings.advanced.updateUpToDate')}
              </span>
            )}
          </div>
          <div className="flex flex-col md:flex-row md:items-center justify-between gap-lg">
            <div className="flex-1">
              <p className="text-on-surface-variant text-body-sm mb-md">{t('settings.advanced.updateDesc')}</p>
              {updateInfo && (
                <p className="text-on-surface-variant text-label-sm">
                  {intl.formatMessage({ id: 'settings.advanced.updateCurrent' }, { version: updateInfo.currentVersion })}
                </p>
              )}
              {updateInfo?.error && (
                <p className="text-error text-label-sm mt-xs">{updateInfo.error}</p>
              )}
            </div>
            <div className="flex items-center gap-md shrink-0">
              {updateInfo && !updateInfo.error && (
                <Button
                  variant="ghost"
                  className="flex items-center gap-xs text-link font-label-md text-[14px] hover:underline cursor-pointer"
                  onClick={handleOpenReleasePage}
                >
                  <span className="material-symbols-outlined icon-sm">open_in_new</span>
                  {t('settings.advanced.updateOpenPage')}
                </Button>
              )}
              <Button
                className="px-xl py-md bg-primary text-on-primary rounded-xl font-label-md text-[14px] font-bold hover:bg-primary/90 shadow-md active:scale-[0.98] transition-all whitespace-nowrap cursor-pointer"
                onClick={handleCheckUpdate}
                disabled={checkingUpdate}
              >
                {checkingUpdate ? t('settings.advanced.updateChecking') : t('settings.advanced.updateCheckButton')}
              </Button>
            </div>
          </div>
        </div>

        {/* Developer Options */}
        <div className="bg-surface-container-lowest p-lg rounded-xl shadow-sm border border-outline-variant/30 lg:col-span-2 group hover:shadow-md transition-shadow">
          <div className="flex items-center gap-md mb-md">
            <div className="p-2 bg-tertiary/10 rounded-lg text-tertiary flex items-center justify-center">
              <span className="material-symbols-outlined">terminal</span>
            </div>
            <h3 className="font-headline-md text-[24px] font-bold text-on-surface">{t('settings.advanced.devOptions')}</h3>
          </div>
          <div className="flex flex-col md:flex-row md:items-center justify-between gap-lg">
            <div className="flex-1">
              <p className="text-on-surface-variant text-body-sm mb-md">{t('settings.advanced.devOptionsDesc')}</p>
              <div className="flex items-center gap-md">
                <Button variant="ghost" className="flex items-center gap-xs text-link font-label-md text-[14px] hover:underline cursor-pointer" onClick={() => setShowLogs(true)}>
                  <span className="material-symbols-outlined icon-sm">description</span>
                  {t('settings.advanced.viewLogs')}
                </Button>
                <span className="text-outline-variant">|</span>
                <Button variant="ghost" className="flex items-center gap-xs text-link font-label-md text-[14px] hover:underline cursor-pointer" onClick={() => setShowApiKeys(true)}>
                  <span className="material-symbols-outlined icon-sm">api</span>
                  {t('settings.advanced.manageApiKeys')}
                </Button>
              </div>
            </div>
            <div className="flex items-center gap-md bg-surface-container-low p-md rounded-xl border border-outline-variant/20 shrink-0">
              <span className="font-label-md text-[14px] text-on-surface">{t('settings.advanced.enableDebug')}</span>
              <Switch checked={debugConsole} onCheckedChange={v => handleToggle('debug_console', v, setDebugConsole)} />
            </div>
          </div>
        </div>

        {/* Critical System Reset */}
        <div className="lg:col-span-2 border-2 border-error/20 bg-error/5 p-lg rounded-xl mt-sm relative overflow-hidden">
          <div className="flex flex-col md:flex-row items-start md:items-center justify-between gap-lg relative z-raised">
            <div className="flex items-start gap-md">
              <div className="p-2 bg-error/10 rounded-lg text-error shrink-0 flex items-center justify-center">
                <span className="material-symbols-outlined">warning</span>
              </div>
              <div>
                <h3 className="font-headline-md text-[24px] font-bold text-error mb-1">{t('settings.advanced.resetTitle')}</h3>
                <p className="text-on-surface-variant text-body-sm">{t('settings.advanced.resetDesc')}</p>
              </div>
            </div>
            <Button
              className="px-xl py-md bg-error text-on-error rounded-xl font-label-md text-[14px] font-bold hover:bg-error/90 shadow-md active:scale-[0.98] transition-all whitespace-nowrap cursor-pointer"
              onClick={() => setShowResetConfirm(true)}
              disabled={resetting}
            >
              {resetting ? t('settings.advanced.resetting') : t('settings.advanced.resetButton')}
            </Button>
          </div>
        </div>
      </div>

      {/* System Logs Modal */}
      <Modal open={showLogs} onClose={() => setShowLogs(false)} title={t('settings.advanced.systemLogs')} size="2xl">
        <div className="px-xl pb-xl">
          <div className="bg-surface-container-high rounded-xl p-md font-mono text-label-sm text-on-surface-variant max-h-[50vh] overflow-y-auto">
            <p>Shannon Desktop v0.1.0</p>
            <p>{t('settings.advanced.logsHelp')}</p>
            <p className="mt-sm opacity-60">{t('settings.advanced.logsVerbose')}</p>
          </div>
        </div>
      </Modal>

      {/* API Keys Modal */}
      <Modal open={showApiKeys} onClose={() => setShowApiKeys(false)} title={t('settings.advanced.manageApiKeys')} size="lg">
        <div className="px-xl pb-xl">
          <p className="text-body-sm text-on-surface-variant mb-md">{t('settings.advanced.apiKeysHelp')}</p>
          <Button className="w-full py-md bg-primary text-on-primary rounded-xl font-label-md cursor-pointer" onClick={() => setShowApiKeys(false)}>
            {t('settings.advanced.goToModelSettings')}
          </Button>
        </div>
      </Modal>

      {/* Clear Cache Confirmation */}
      <ConfirmDialog
        open={showClearConfirm}
        title={t('settings.advanced.clearSessionCache')}
        message={t('settings.advanced.clearDesc')}
        confirmLabel={t('settings.advanced.clearCache')}
        busyLabel={t('settings.advanced.clearing')}
        cancelLabel={t('settings.advanced.cancel')}
        busy={clearing}
        onConfirm={handleClearCache}
        onCancel={() => setShowClearConfirm(false)}
      />

      {/* Factory Reset Confirmation */}
      <ConfirmDialog
        open={showResetConfirm}
        title={t('settings.advanced.factoryReset')}
        message={t('settings.advanced.factoryResetDesc')}
        confirmLabel={t('settings.advanced.reset')}
        busyLabel={t('settings.advanced.resetting')}
        cancelLabel={t('settings.advanced.cancel')}
        destructive
        busy={resetting}
        onConfirm={handleFactoryReset}
        onCancel={() => setShowResetConfirm(false)}
      />

      <SkillApprovalModal
        open={approvalOpen}
        candidate={candidates[candidateIndex] ?? null}
        onClose={() => setApprovalOpen(false)}
        onApproved={() => advanceCandidate()}
        onRejected={() => advanceCandidate()}
      />
    </div>
  )
}
