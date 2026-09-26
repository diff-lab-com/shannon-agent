import { useState, useEffect } from 'react'
import { Spinner } from '@/components/ui/loading-state'
import { useNavigate } from 'react-router-dom'
import { toast } from 'sonner'
import { useIntl } from 'react-intl'
import { Button } from '@/components/ui/button'
import { cn } from '@/lib/utils'
import { useCatalog } from '@/context/CatalogContext'
import { useI18n, SUPPORTED_LOCALES, type Locale } from '@/i18n'
import { useNotification } from '@/hooks/useNotification'
import * as api from '@/lib/tauri-api'
import { toastError } from '@/lib/errorToast'
import { readDensityPref, setDensityPref, type DensityPref } from '@/lib/density'
import { getLinkTarget, setLinkTarget as setLinkTargetPref, type LinkTarget } from '@/lib/openLink'
import { Switch } from '@/components/ui/switch'
import { useArtifact } from '@/components/artifact/ArtifactContext'
import type { ApprovalMode } from '@/types'
import { WELCOME_SEEN_KEY } from '@/pages/Welcome'
import MigrationWizard from '@/components/migration/MigrationWizard'
import PersonaPackSettings from './PersonaPackSettings'
import { FeedbackSummaryCard } from './FeedbackSummaryCard'

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
  // P2-⑧/D6 display density: 'auto' follows the sidebar mode (Advanced →
  // Compact); an explicit choice overrides and persists.
  const [density, setDensityState] = useState<DensityPref>(readDensityPref)
  const handleDensityChange = (d: DensityPref) => {
    setDensityState(d)
    setDensityPref(d)
    // Re-apply immediately: resolve against the current sidebar mode.
    import('@/lib/density').then(m => m.initDensity())
  }
  const intl = useIntl()
  const navigate = useNavigate()
  const t = (id: string) => intl.formatMessage({ id })
  // Batch D4: artifact auto-open — app-scoped ArtifactContext (Settings and
  // the Chat dock share the same live preference).
  const { autoOpen, setAutoOpen: setArtifactAutoOpen } = useArtifact()
  // P1-E (decision §5-6): default destination for external links.
  const [linkTarget, setLinkTargetState] = useState<LinkTarget>(() => getLinkTarget())
  const setLinkTarget = (next: LinkTarget) => {
    setLinkTargetState(next)
    setLinkTargetPref(next)
  }
  const { locale, setLocale } = useI18n()
  const notify = useNotification()
  const [approvalMode, setApprovalMode] = useState<number>(2) // default to "plan"
  const [saving, setSaving] = useState(false)
  const [testingNotification, setTestingNotification] = useState(false)
  // P1-6 — migration wizard (import from Claude Code / ZCode).
  const [migrationOpen, setMigrationOpen] = useState(false)

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
    setSaving(true)
    try {
      await api.configure({ key: 'approval_mode', value: APPROVAL_MODE_KEYS[idx].value })
      await refreshConfig()
      // Approval mode is a safety switch — the UI only moves after the
      // write has actually landed (P1-10: no optimistic update here).
      setApprovalMode(idx)
      toast.success(intl.formatMessage({ id: 'settings.general.approvalMode.updated' }, { label: t(APPROVAL_MODE_KEYS[idx].labelKey) }))
    } catch (e) { toastError(t('settings.general.approvalMode.updateFailed'), e) }
    setSaving(false)
  }

  const currentMode = APPROVAL_MODE_KEYS[approvalMode]

  return (
    <div className="max-w-3xl">
      <p className="font-body-md text-on-surface-variant mb-md">{t('settings.general.subheader')}</p>

      <div className="space-y-lg">
        {/* Autonomy Level */}
        <section className="bg-surface-container-lowest rounded-xl border border-outline-variant/30 p-xl shadow-sm transition-all hover:shadow-md">
          <div className="flex items-center gap-md mb-xs">
            <span className="material-symbols-outlined text-primary" style={{fontVariationSettings: "'FILL' 1"}}>auto_awesome</span>
            <h3 className="font-headline-md text-headline-md">{t('settings.general.approvalMode.title')}</h3>
            {saving && <Spinner className="text-primary text-[18px]" />}
          </div>
          <p className="font-body-sm text-on-surface-variant mb-xl">
            {intl.formatMessage({ id: 'settings.general.approvalMode.current' }, {
              label: t(currentMode.labelKey),
              description: t(currentMode.descriptionKey),
            })}
          </p>
          {/* Segmented control replaces the old range slider whose 5 label
              columns overlapped at common widths (audit P0 §3.10). Equal
              flex segments carry the short label only; the selected mode's
              description moves to a single helper line below. */}
          <div role="radiogroup" aria-label={intl.formatMessage({ id: 'settings.general.approvalMode.sliderAria' })}>
            <div className="flex rounded-xl bg-surface-container-low p-1 gap-1 border border-outline-variant/30">
              {APPROVAL_MODE_KEYS.map((m, i) => (
                <button
                  key={m.value}
                  type="button"
                  role="radio"
                  aria-checked={i === approvalMode}
                  onClick={() => handleModeChange(i)}
                  className={cn(
                    'flex-1 min-w-0 px-1 py-sm rounded-lg font-label-md text-center cursor-pointer transition-all duration-200',
                    'focus-visible:outline focus-visible:outline-2 focus-visible:outline-primary',
                    i === approvalMode
                      ? 'bg-primary text-on-primary font-bold shadow-e1'
                      : 'text-on-surface-variant hover:text-primary hover:bg-surface-container-high',
                  )}
                >
                  <span className="block truncate">{t(m.labelKey)}</span>
                </button>
              ))}
            </div>
            <p className="font-body-sm text-on-surface-variant mt-sm px-1">
              {t(currentMode.descriptionKey)}
            </p>
          </div>
        </section>

        {/* Language */}
        <section className="bg-surface-container-lowest rounded-xl border border-outline-variant/30 p-xl shadow-sm transition-all hover:shadow-md">
          <div className="flex items-center gap-md mb-xs">
            <span className="material-symbols-outlined text-primary" style={{fontVariationSettings: "'FILL' 1"}}>translate</span>
            <h3 className="font-headline-md text-headline-md">{intl.formatMessage({ id: 'settings.language.label' })}</h3>
          </div>
          <p className="font-body-sm text-on-surface-variant mb-xl">{intl.formatMessage({ id: 'settings.language.help' })}</p>
          <div className="flex flex-wrap gap-sm">
            {SUPPORTED_LOCALES.map(opt => (
              <Button
                key={opt.id}
                variant={locale === opt.id ? 'default' : 'outline'}
                onClick={() => handleLocaleChange(opt.id)}
                aria-pressed={locale === opt.id}
                className={cn(
                  'px-lg py-sm rounded-lg font-label-md cursor-pointer transition-all focus-visible:outline focus-visible:outline-2 focus-visible:outline-primary',
                  locale === opt.id
                    ? 'bg-primary text-on-primary'
                    : 'bg-surface-container-low text-on-surface hover:bg-surface-container-high border border-outline-variant/50',
                )}
              >
                {intl.formatMessage({ id: opt.labelKey })}
              </Button>
            ))}
          </div>
        </section>

        {/* P2-⑧ Display density */}
        <section className="bg-surface-container-lowest rounded-xl border border-outline-variant/30 p-xl shadow-sm transition-all hover:shadow-md">
          <div className="flex items-center gap-md mb-xs">
            <span className="material-symbols-outlined text-primary" style={{fontVariationSettings: "'FILL' 1"}}>format_line_spacing</span>
            <h3 className="font-headline-md text-headline-md">{intl.formatMessage({ id: 'settings.density.title' })}</h3>
          </div>
          <p className="font-body-sm text-on-surface-variant mb-xl">{intl.formatMessage({ id: 'settings.density.help' })}</p>
          <div className="flex flex-wrap gap-sm">
            {([
              { id: 'auto' as const, labelKey: 'settings.density.auto' },
              { id: 'comfortable' as const, labelKey: 'settings.density.comfortable' },
              { id: 'compact' as const, labelKey: 'settings.density.compact' },
            ]).map(opt => (
              <Button
                key={opt.id}
                variant={density === opt.id ? 'default' : 'outline'}
                onClick={() => handleDensityChange(opt.id)}
                aria-pressed={density === opt.id}
                className={cn(
                  'px-lg py-sm rounded-lg font-label-md cursor-pointer transition-all focus-visible:outline focus-visible:outline-2 focus-visible:outline-primary',
                  density === opt.id
                    ? 'bg-primary text-on-primary'
                    : 'bg-surface-container-low text-on-surface hover:bg-surface-container-high border border-outline-variant/50',
                )}
              >
                {intl.formatMessage({ id: opt.labelKey })}
              </Button>
            ))}
          </div>
        </section>

        {/* Session Info */}
        <section className="bg-surface-container-lowest rounded-xl border border-outline-variant/30 p-xl shadow-sm">
          <h3 className="font-headline-md text-headline-md mb-md">{t('settings.general.sessionInfo.title')}</h3>
          <div className="space-y-sm">
            {/* Batch D4: artifact auto-open preference (recovered from the
                retired ArtifactPanel — now the only surface for the toggle). */}
            <div className="flex justify-between items-center py-sm gap-md">
              <span className="min-w-0">
                <span className="font-label-md text-on-surface block">{t('settings.general.artifactAutoOpen.title')}</span>
                <span className="font-label-sm text-on-surface-variant block">{t('settings.general.artifactAutoOpen.description')}</span>
              </span>
              <Switch
                checked={autoOpen}
                onCheckedChange={setArtifactAutoOpen}
                aria-label={t('settings.general.artifactAutoOpen.title')}
              />
            </div>
            {/* P1-E (decision §5-6): where plain clicks on external links land.
                Alt+click / right-click always offer both destinations. */}
            <div className="flex justify-between items-center py-sm gap-md">
              <span className="min-w-0">
                <span className="font-label-md text-on-surface block">{t('settings.general.linkTarget')}</span>
                <span className="font-label-sm text-on-surface-variant block">{t('settings.general.linkTarget.desc')}</span>
              </span>
              <select
                value={linkTarget}
                onChange={e => setLinkTarget(e.target.value as LinkTarget)}
                aria-label={t('settings.general.linkTarget')}
                className="font-label-md text-on-surface bg-surface-container rounded-lg px-sm py-xs border border-outline-variant/30 cursor-pointer"
              >
                <option value="panel">{t('settings.general.linkTarget.panel')}</option>
                <option value="browser">{t('settings.general.linkTarget.browser')}</option>
              </select>
            </div>
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
          <Button
            onClick={handleTestNotification}
            disabled={testingNotification}
            className="px-lg py-sm rounded-lg font-label-md cursor-pointer transition-all bg-primary text-on-primary hover:bg-primary/90 disabled:opacity-50 disabled:cursor-not-allowed focus-visible:outline focus-visible:outline-2 focus-visible:outline-primary"
          >
            {testingNotification
              ? intl.formatMessage({ id: 'settings.notifications.sending' })
              : intl.formatMessage({ id: 'settings.notifications.testButton' })}
          </Button>
        </section>

        {/* PM-12: persisted message ratings, aggregated per session */}
        <FeedbackSummaryCard />

        {/* P1-6 — migration wizard entry (import from Claude Code / ZCode) */}
        <section className="bg-surface-container-lowest rounded-xl border border-outline-variant/30 p-xl shadow-sm">
          <div className="flex items-center gap-md mb-xs">
            <span className="material-symbols-outlined text-primary" style={{fontVariationSettings: "'FILL' 1"}}>move_in</span>
            <h3 className="font-headline-md text-headline-md">{t('settings.migration.title')}</h3>
          </div>
          <p className="font-body-sm text-on-surface-variant mb-xl">{t('settings.migration.desc')}</p>
          <Button
            variant="outline"
            onClick={() => setMigrationOpen(true)}
            data-testid="settings-migration-open"
            className="px-lg py-sm rounded-lg font-label-md cursor-pointer transition-all bg-surface-container-low hover:bg-surface-container-high border border-outline-variant/50 text-on-surface focus-visible:outline focus-visible:outline-2 focus-visible:outline-primary"
          >
            {t('settings.migration.button')}
          </Button>
        </section>

        {/* P2-2 — persona/profile pack (one-file export & import) */}
        <PersonaPackSettings />

        {/* Re-run setup wizard */}
        <section className="bg-surface-container-lowest rounded-xl border border-outline-variant/30 p-xl shadow-sm">
          <div className="flex items-center gap-md mb-xs">
            <span className="material-symbols-outlined text-primary" style={{fontVariationSettings: "'FILL' 1"}}>refresh</span>
            <h3 className="font-headline-md text-headline-md">{t('settings.general.rerunWizard.title')}</h3>
          </div>
          <p className="font-body-sm text-on-surface-variant mb-xl">{t('settings.general.rerunWizard.description')}</p>
          <Button
            variant="outline"
            onClick={handleRerunWizard}
            className="px-lg py-sm rounded-lg font-label-md cursor-pointer transition-all bg-surface-container-low hover:bg-surface-container-high border border-outline-variant/50 text-on-surface focus-visible:outline focus-visible:outline-2 focus-visible:outline-primary"
          >
            {t('settings.general.rerunWizard.button')}
          </Button>
        </section>
      </div>

      {migrationOpen && (
        <MigrationWizard open={migrationOpen} onClose={() => setMigrationOpen(false)} />
      )}
    </div>
  )
}
