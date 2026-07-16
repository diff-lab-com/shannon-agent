import { useState, useEffect } from 'react'
import { useNavigate } from 'react-router-dom'
import { toast } from 'sonner'
import { useIntl } from 'react-intl'
import { useCatalog } from '@/context/CatalogContext'
import { useI18n, SUPPORTED_LOCALES, type Locale } from '@/i18n'
import { useNotification } from '@/hooks/useNotification'
import * as api from '@/lib/tauri-api'
import { toastError } from '@/lib/errorToast'
import type { ApprovalMode } from '@/types'
import { WELCOME_SEEN_KEY } from '@/pages/Welcome'

type ApprovalModeKey = ApprovalMode

const APPROVAL_MODE_KEYS: { value: ApprovalModeKey; labelKey: string; descriptionKey: string }[] = [
  { value: 'suggest', labelKey: 'settings.general.approvalMode.suggest.label', descriptionKey: 'settings.general.approvalMode.suggest.description' },
  { value: 'confirm', labelKey: 'settings.general.approvalMode.confirm.label', descriptionKey: 'settings.general.approvalMode.confirm.description' },
  { value: 'plan', labelKey: 'settings.general.approvalMode.plan.label', descriptionKey: 'settings.general.approvalMode.plan.description' },
  { value: 'auto_edit', labelKey: 'settings.general.approvalMode.autoEdit.label', descriptionKey: 'settings.general.approvalMode.autoEdit.description' },
  { value: 'full_auto', labelKey: 'settings.general.approvalMode.fullAuto.label', descriptionKey: 'settings.general.approvalMode.fullAuto.description' },
]

export default function GeneralSettings() {
  const { config, refreshConfig } = useCatalog()
  const intl = useIntl()
  const navigate = useNavigate()
  const t = (id: string) => intl.formatMessage({ id })
  const { locale, setLocale } = useI18n()
  const notify = useNotification()
  const [approvalMode, setApprovalMode] = useState<number>(2) // default to "plan"
  const [saving, setSaving] = useState(false)
  const [testingNotification, setTestingNotification] = useState(false)

  const handleRerunWizard = () => {
    window.localStorage.removeItem(WELCOME_SEEN_KEY)
    navigate('/welcome')
  }

  const handleTestNotification = async () => {
    setTestingNotification(true)
    try {
      await notify({
        title: intl.formatMessage({ id: 'settings.notifications.testTitle' }),
        body: intl.formatMessage({ id: 'settings.notifications.testBody' }),
        level: 'info',
      })
      toast.success(intl.formatMessage({ id: 'settings.notifications.testSent' }))
    } catch (e) {
      toastError(intl.formatMessage({ id: 'settings.notifications.testFailed' }), e)
    }
    setTestingNotification(false)
  }

  const handleLocaleChange = (next: Locale) => {
    setLocale(next)
    toast.success(intl.formatMessage({ id: 'settings.language.label' }))
  }

  useEffect(() => {
    if (config?.approval_mode) {
      const idx = APPROVAL_MODE_KEYS.findIndex(m => m.value === config.approval_mode)
      if (idx >= 0) setApprovalMode(idx)
    }
  }, [config])

  const handleModeChange = async (idx: number) => {
    setApprovalMode(idx)
    setSaving(true)
    try {
      await api.configure({ key: 'approval_mode', value: APPROVAL_MODE_KEYS[idx].value })
      await refreshConfig()
      toast.success(intl.formatMessage({ id: 'settings.general.approvalMode.updated' }, { label: t(APPROVAL_MODE_KEYS[idx].labelKey) }))
    } catch (e) { toastError(t('settings.general.approvalMode.updateFailed'), e) }
    setSaving(false)
  }

  const currentMode = APPROVAL_MODE_KEYS[approvalMode]

  return (
    <div className="max-w-3xl">
      <header className="mb-xl">
        <h2 className="font-headline-lg text-headline-lg text-on-surface mb-xs">{t('settings.general.header')}</h2>
        <p className="font-body-md text-on-surface-variant">{t('settings.general.subheader')}</p>
      </header>

      <div className="space-y-lg">
        {/* Autonomy Level */}
        <section className="bg-surface-container-lowest rounded-xl border border-outline-variant/30 p-xl shadow-sm transition-all hover:shadow-md">
          <div className="flex items-center gap-md mb-xs">
            <span className="material-symbols-outlined text-primary" style={{fontVariationSettings: "'FILL' 1"}}>auto_awesome</span>
            <h3 className="font-headline-md text-headline-md">{t('settings.general.approvalMode.title')}</h3>
            {saving && <span className="material-symbols-outlined text-primary animate-spin text-[18px]">progress_activity</span>}
          </div>
          <p className="font-body-sm text-on-surface-variant mb-xl">
            {intl.formatMessage({ id: 'settings.general.approvalMode.current' }, {
              label: t(currentMode.labelKey),
              description: t(currentMode.descriptionKey),
            })}
          </p>
          <div className="space-y-sm">
            <input
              className="w-full appearance-none bg-outline-variant/30 h-1 rounded-full cursor-pointer outline-none slider-thumb-primary"
              max={APPROVAL_MODE_KEYS.length - 1} min={0} type="range" value={approvalMode}
              aria-valuenow={approvalMode} aria-valuemin={0} aria-valuemax={APPROVAL_MODE_KEYS.length - 1}
              onChange={e => handleModeChange(Number(e.target.value))}
            />
            <div className="flex justify-between font-label-sm text-outline px-1">
              {APPROVAL_MODE_KEYS.map((m, i) => (
                <button
                  key={m.value}
                  onClick={() => handleModeChange(i)}
                  className={`text-center cursor-pointer transition-colors ${i === approvalMode ? 'text-primary font-bold' : 'text-on-surface-variant hover:text-primary'}`}
                >
                  <p className="font-bold">{t(m.labelKey)}</p>
                  <p className="text-[10px]">{t(m.descriptionKey)}</p>
                </button>
              ))}
            </div>
          </div>
        </section>

        {/* Language */}
        <section className="bg-surface-container-lowest rounded-xl border border-outline-variant/30 p-xl shadow-sm transition-all hover:shadow-md">
          <div className="flex items-center gap-md mb-xs">
            <span className="material-symbols-outlined text-primary" style={{fontVariationSettings: "'FILL' 1"}}>translate</span>
            <h3 className="font-headline-md text-headline-md">{intl.formatMessage({ id: 'settings.language.label' })}</h3>
          </div>
          <p className="font-body-sm text-on-surface-variant mb-xl">{intl.formatMessage({ id: 'settings.language.help' })}</p>
          <div className="flex gap-sm">
            {SUPPORTED_LOCALES.map(opt => (
              <button
                key={opt.id}
                onClick={() => handleLocaleChange(opt.id)}
                aria-pressed={locale === opt.id}
                className={`px-lg py-sm rounded-lg font-label-md cursor-pointer transition-all focus-visible:outline focus-visible:outline-2 focus-visible:outline-primary ${
                  locale === opt.id
                    ? 'bg-primary text-on-primary'
                    : 'bg-surface-container-low text-on-surface hover:bg-surface-container-high border border-outline-variant/50'
                }`}
              >
                {intl.formatMessage({ id: opt.labelKey })}
              </button>
            ))}
          </div>
        </section>

        {/* Session Info */}
        <section className="bg-surface-container-lowest rounded-xl border border-outline-variant/30 p-xl shadow-sm">
          <h3 className="font-headline-md text-headline-md mb-md">{t('settings.general.sessionInfo.title')}</h3>
          <div className="space-y-sm">
            <div className="flex justify-between items-center py-sm">
              <span className="font-label-md text-on-surface-variant">{t('settings.general.sessionInfo.activeProvider')}</span>
              <span className="font-label-md text-on-surface font-bold">{config?.provider ?? t('settings.general.sessionInfo.notConfigured')}</span>
            </div>
            <div className="flex justify-between items-center py-sm">
              <span className="font-label-md text-on-surface-variant">{t('settings.general.sessionInfo.model')}</span>
              <span className="font-label-md text-on-surface font-bold">{config?.model ?? t('settings.general.sessionInfo.notConfigured')}</span>
            </div>
            <div className="flex justify-between items-center py-sm">
              <span className="font-label-md text-on-surface-variant">{t('settings.general.sessionInfo.workingDir')}</span>
              <span className="font-label-md text-on-surface font-bold font-mono text-sm truncate max-w-[300px]">{config?.working_dir ?? t('settings.general.sessionInfo.notSet')}</span>
            </div>
          </div>
        </section>

        {/* Notifications */}
        <section className="bg-surface-container-lowest rounded-xl border border-outline-variant/30 p-xl shadow-sm">
          <div className="flex items-center gap-md mb-xs">
            <span className="material-symbols-outlined text-primary" style={{fontVariationSettings: "'FILL' 1"}}>notifications</span>
            <h3 className="font-headline-md text-headline-md">{intl.formatMessage({ id: 'settings.notifications.label' })}</h3>
          </div>
          <p className="font-body-sm text-on-surface-variant mb-xl">{intl.formatMessage({ id: 'settings.notifications.help' })}</p>
          <button
            onClick={handleTestNotification}
            disabled={testingNotification}
            className="px-lg py-sm rounded-lg font-label-md cursor-pointer transition-all bg-primary text-on-primary hover:bg-primary/90 disabled:opacity-50 disabled:cursor-not-allowed focus-visible:outline focus-visible:outline-2 focus-visible:outline-primary"
          >
            {testingNotification
              ? intl.formatMessage({ id: 'settings.notifications.sending' })
              : intl.formatMessage({ id: 'settings.notifications.testButton' })}
          </button>
        </section>

        {/* Re-run setup wizard */}
        <section className="bg-surface-container-lowest rounded-xl border border-outline-variant/30 p-xl shadow-sm">
          <div className="flex items-center gap-md mb-xs">
            <span className="material-symbols-outlined text-primary" style={{fontVariationSettings: "'FILL' 1"}}>refresh</span>
            <h3 className="font-headline-md text-headline-md">{t('settings.general.rerunWizard.title')}</h3>
          </div>
          <p className="font-body-sm text-on-surface-variant mb-xl">{t('settings.general.rerunWizard.description')}</p>
          <button
            onClick={handleRerunWizard}
            className="px-lg py-sm rounded-lg font-label-md cursor-pointer transition-all bg-surface-container-low hover:bg-surface-container-high border border-outline-variant/50 text-on-surface focus-visible:outline focus-visible:outline-2 focus-visible:outline-primary"
          >
            {t('settings.general.rerunWizard.button')}
          </button>
        </section>
      </div>
    </div>
  )
}
