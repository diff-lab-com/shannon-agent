import { useState, useRef } from 'react'
import { useIntl } from 'react-intl'
import { toast } from 'sonner'
import { Button } from '@/components/ui/button'
import { Switch } from '@/components/ui/switch'
import { useApp } from '@/context/AppContext'
import { useModalFocus } from '@/hooks/useModalFocus'
import * as api from '@/lib/tauri-api'
import { toastError } from '@/lib/errorToast'

export default function AdvancedSettings() {
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })
  const { refreshConfig, config } = useApp()
  const [memoryEnabled, setMemoryEnabled] = useState(config?.memory_enabled ?? true)
  const [telemetryEnabled, setTelemetryEnabled] = useState(config?.telemetry_enabled ?? false)
  const [encryptionEnabled, setEncryptionEnabled] = useState(config?.encryption_enabled ?? true)
  const [debugConsole, setDebugConsole] = useState(config?.debug_console ?? false)
  const [skillLoopEnabled, setSkillLoopEnabled] = useState(config?.skill_loop_enabled ?? false)
  const [clearing, setClearing] = useState(false)
  const [resetting, setResetting] = useState(false)
  const [showLogs, setShowLogs] = useState(false)
  const [showApiKeys, setShowApiKeys] = useState(false)
  const [showResetConfirm, setShowResetConfirm] = useState(false)
  const [showClearConfirm, setShowClearConfirm] = useState(false)

  const logsRef = useRef<HTMLDivElement>(null)
  useModalFocus(showLogs, logsRef)
  const apiKeysRef = useRef<HTMLDivElement>(null)
  useModalFocus(showApiKeys, apiKeysRef)
  const clearConfirmRef = useRef<HTMLDivElement>(null)
  useModalFocus(showClearConfirm, clearConfirmRef)
  const resetConfirmRef = useRef<HTMLDivElement>(null)
  useModalFocus(showResetConfirm, resetConfirmRef)

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
          </div>
          <p className="text-on-surface-variant text-body-sm mb-lg">{t('settings.skillLoop.description')}</p>
          <div className="flex items-center justify-between gap-md">
            <div>
              <div className="font-label-md text-[14px] text-on-surface font-semibold mb-1">{t('settings.skillLoop.enabled')}</div>
              <div className="font-label-sm text-[12px] text-on-surface-variant leading-tight">{t('settings.skillLoop.enabledDesc')}</div>
            </div>
            <Switch checked={skillLoopEnabled} onCheckedChange={v => handleToggle('skill_loop_enabled', v, setSkillLoopEnabled)} className="shrink-0" />
          </div>
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
              {clearing ? <span className="material-symbols-outlined animate-spin mr-sm text-[18px]">progress_activity</span> : null}
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
                <Button variant="ghost" className="flex items-center gap-xs text-primary font-label-md text-[14px] hover:underline cursor-pointer" onClick={() => setShowLogs(true)}>
                  <span className="material-symbols-outlined icon-sm">description</span>
                  {t('settings.advanced.viewLogs')}
                </Button>
                <span className="text-outline-variant">|</span>
                <Button variant="ghost" className="flex items-center gap-xs text-primary font-label-md text-[14px] hover:underline cursor-pointer" onClick={() => setShowApiKeys(true)}>
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
          <div className="flex flex-col md:flex-row items-start md:items-center justify-between gap-lg relative z-10">
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
      {showLogs && (
        <div ref={logsRef} role="dialog" aria-modal="true" className="fixed inset-0 bg-black/30 flex items-center justify-center z-50" onClick={() => setShowLogs(false)} onKeyDown={e => { if (e.key === 'Escape') setShowLogs(false) }}>
          <div className="bg-surface-container-lowest rounded-2xl p-xl max-w-2xl w-full mx-lg shadow-2xl max-h-[80vh] overflow-y-auto" onClick={e => e.stopPropagation()}>
            <div className="flex justify-between items-center mb-lg">
              <h3 className="font-headline-md text-on-surface">{t('settings.advanced.systemLogs')}</h3>
              <Button variant="ghost" className="cursor-pointer" onClick={() => setShowLogs(false)}>
                <span className="material-symbols-outlined">close</span>
              </Button>
            </div>
            <div className="bg-surface-container-high rounded-xl p-md font-mono text-label-sm text-on-surface-variant max-h-[50vh] overflow-y-auto">
              <p>Shannon Desktop v0.1.0</p>
              <p>{t('settings.advanced.logsHelp')}</p>
              <p className="mt-sm opacity-60">{t('settings.advanced.logsVerbose')}</p>
            </div>
          </div>
        </div>
      )}

      {/* API Keys Modal */}
      {showApiKeys && (
        <div ref={apiKeysRef} role="dialog" aria-modal="true" className="fixed inset-0 bg-black/30 flex items-center justify-center z-50" onClick={() => setShowApiKeys(false)} onKeyDown={e => { if (e.key === 'Escape') setShowApiKeys(false) }}>
          <div className="bg-surface-container-lowest rounded-2xl p-xl max-w-lg w-full mx-lg shadow-2xl" onClick={e => e.stopPropagation()}>
            <div className="flex justify-between items-center mb-lg">
              <h3 className="font-headline-md text-on-surface">{t('settings.advanced.manageApiKeys')}</h3>
              <Button variant="ghost" className="cursor-pointer" onClick={() => setShowApiKeys(false)}>
                <span className="material-symbols-outlined">close</span>
              </Button>
            </div>
            <p className="text-body-sm text-on-surface-variant mb-md">{t('settings.advanced.apiKeysHelp')}</p>
            <Button className="w-full py-md bg-primary text-on-primary rounded-xl font-label-md cursor-pointer" onClick={() => setShowApiKeys(false)}>
              {t('settings.advanced.goToModelSettings')}
            </Button>
          </div>
        </div>
      )}

      {/* Clear Cache Confirmation */}
      {showClearConfirm && (
        <div ref={clearConfirmRef} role="dialog" aria-modal="true" className="fixed inset-0 bg-black/30 backdrop-blur-sm flex items-center justify-center z-50" onClick={() => setShowClearConfirm(false)} onKeyDown={e => { if (e.key === 'Escape') setShowClearConfirm(false) }}>
          <div className="bg-surface-container-lowest rounded-2xl p-xl shadow-xl border border-outline-variant/30 max-w-sm w-full mx-md" onClick={e => e.stopPropagation()}>
            <div className="flex items-center gap-sm mb-md">
              <span className="material-symbols-outlined text-secondary text-[24px]">cleaning_services</span>
              <h3 className="font-headline-md text-on-surface">{t('settings.advanced.clearSessionCache')}</h3>
            </div>
            <p className="text-body-md text-on-surface-variant mb-lg">{t('settings.advanced.clearDesc')}</p>
            <div className="flex justify-end gap-sm">
              <Button className="px-lg py-sm rounded-xl text-on-surface-variant hover:bg-surface-container" onClick={() => setShowClearConfirm(false)}>{t('settings.advanced.cancel')}</Button>
              <Button className="px-lg py-sm rounded-xl bg-secondary text-on-secondary hover:bg-secondary/90" onClick={handleClearCache} disabled={clearing}>{clearing ? t('settings.advanced.clearing') : t('settings.advanced.clearCache')}</Button>
            </div>
          </div>
        </div>
      )}

      {/* Factory Reset Confirmation */}
      {showResetConfirm && (
        <div ref={resetConfirmRef} role="dialog" aria-modal="true" className="fixed inset-0 bg-black/30 backdrop-blur-sm flex items-center justify-center z-50" onClick={() => setShowResetConfirm(false)} onKeyDown={e => { if (e.key === 'Escape') setShowResetConfirm(false) }}>
          <div className="bg-surface-container-lowest rounded-2xl p-xl shadow-xl border border-outline-variant/30 max-w-sm w-full mx-md" onClick={e => e.stopPropagation()}>
            <div className="flex items-center gap-sm mb-md">
              <span className="material-symbols-outlined text-error text-[24px]">warning</span>
              <h3 className="font-headline-md text-on-surface">{t('settings.advanced.factoryReset')}</h3>
            </div>
            <p className="text-body-md text-on-surface-variant mb-lg">{t('settings.advanced.factoryResetDesc')}</p>
            <div className="flex justify-end gap-sm">
              <Button className="px-lg py-sm rounded-xl text-on-surface-variant hover:bg-surface-container" onClick={() => setShowResetConfirm(false)}>{t('settings.advanced.cancel')}</Button>
              <Button className="px-lg py-sm rounded-xl bg-error text-on-error hover:bg-error/90" onClick={handleFactoryReset} disabled={resetting}>{resetting ? t('settings.advanced.resetting') : t('settings.advanced.reset')}</Button>
            </div>
          </div>
        </div>
      )}
    </div>
  )
}
