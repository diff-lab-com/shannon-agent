import { useState } from 'react'
import { Spinner } from '@/components/ui/loading-state'
import { useNavigate } from 'react-router-dom'
import { toast } from 'sonner'
import { useIntl } from 'react-intl'
import { Button } from '@/components/ui/button'
import { cn } from '@/lib/utils'
import { useCatalog } from '@/context/CatalogContext'
import { useI18n, SUPPORTED_LOCALES, type Locale } from '@/i18n'
import * as api from '@/lib/tauri-api'
import { toastError } from '@/lib/errorToast'
import { readDensityPref, setDensityPref, type DensityPref } from '@/lib/density'
import { getLinkTarget, setLinkTarget as setLinkTargetPref, type LinkTarget } from '@/lib/openLink'
import { Switch } from '@/components/ui/switch'
import { useArtifact } from '@/components/artifact/ArtifactContext'
import { APPROVAL_MODES, ADVANCED_MODES, approvalModeOption } from '@/lib/approvalModes'
import { WELCOME_SEEN_KEY } from '@/pages/Welcome'
import MigrationWizard from '@/components/migration/MigrationWizard'
import PersonaPackSettings from './PersonaPackSettings'
import { FeedbackSummaryCard } from './FeedbackSummaryCard'

// GB P2-4: the tiers come from the SHARED table (lib/approvalModes) — the
// same values, labels and descriptions the composer's quick switcher
// renders, over the same `approval_mode` config key. Round-2: the READ also
// goes through the shared resolver (approvalModeOption), so out-of-table
// engine values like the factory default "confirm" show the raw value with
// no tier selected instead of masquerading as a pickable tier.

export default function GeneralSettings() {
  const { config, providerStatus, refreshConfig } = useCatalog()
  // Real active-provider label for the Session Info row (falls back to the
  // env-detected provider when no managed connection is active). null = the
  // snapshot says genuinely unconfigured → the row renders its "Not
  // configured" placeholder again.
  const activeProviderLabel = providerStatus
    ? (providerStatus.display_name ?? providerStatus.active_provider_id ?? providerStatus.env_provider)
    : null
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
  const [saving, setSaving] = useState(false)
  // P1-6 — migration wizard (import from Claude Code / ZCode).
  const [migrationOpen, setMigrationOpen] = useState(false)

  const handleRerunWizard = () => {
    window.localStorage.removeItem(WELCOME_SEEN_KEY)
    navigate('/welcome')
  }

  const handleLocaleChange = (next: Locale) => {
    setLocale(next)
    toast.success(intl.formatMessage({ id: 'settings.language.label' }))
  }

  // Round-2 review: the read side goes through the SAME honest resolver the
  // composer pill uses. The factory default is `confirm` — an engine alias
  // of suggest (R3) the four-tier table deliberately does not list — and the
  // old index-based read (findIndex miss → stale `useState(2)` default)
  // showed Permissive: a LOOSER tier than the engine's actual
  // ask-per-action, contradicting the composer's raw "confirm" readout.
  // Derived state instead: in-table values select their radio; out-of-table
  // values select NOTHING and name the raw engine value.
  const currentMode = approvalModeOption(config?.approval_mode)
  const selectedIndex = currentMode.rawLabel == null
    ? APPROVAL_MODES.findIndex(m => m.value === currentMode.value)
    : -1

  const handleModeChange = async (option: (typeof APPROVAL_MODES)[number]) => {
    setSaving(true)
    try {
      await api.configure({ key: 'approval_mode', value: option.value })
      await refreshConfig()
      // Approval mode is a safety switch — the read above re-derives from
      // config once the write has actually landed (P1-10: no optimistic
      // update here, and no separate selection state to go stale).
      toast.success(intl.formatMessage({ id: 'settings.general.approvalMode.updated' }, { label: t(option.labelKey) }))
    } catch (e) { toastError(t('settings.general.approvalMode.updateFailed'), e) }
    setSaving(false)
  }

  return (
    <div className="max-w-narrow">
      <p className="font-body-md text-on-surface-variant mb-md">{t('settings.general.subheader')}</p>

      <div className="space-y-lg">
        {/* Autonomy Level */}
        <section className="bg-surface-container-lowest rounded-xl border border-outline-variant/30 p-xl shadow-e1 transition-all hover:shadow-e2">
          <div className="flex items-center gap-md mb-xs">
            <span className="material-symbols-outlined text-primary" style={{fontVariationSettings: "'FILL' 1"}}>auto_awesome</span>
            <h3 className="font-headline-md text-headline-md">{t('settings.general.approvalMode.title')}</h3>
            {saving && <Spinner className="text-primary text-body-lg" />}
          </div>
          <p className="font-body-sm text-on-surface-variant mb-xl">
            {intl.formatMessage({ id: 'settings.general.approvalMode.current' }, {
              label: currentMode.rawLabel ?? t(currentMode.labelKey),
              description: t(currentMode.descriptionKey),
            })}
          </p>
          {/* Segmented control replaces the old range slider whose 5 label
              columns overlapped at common widths (audit P0 §3.10). Equal
              flex segments carry the short label only; the selected mode's
              description moves to a single helper line below. An out-of-table
              engine value (confirm / dont_ask / …) selects NO segment. */}
          <div role="radiogroup" aria-label={intl.formatMessage({ id: 'settings.general.approvalMode.sliderAria' })}>
            <div className="flex rounded-xl bg-surface-container-low p-xs gap-xs border border-outline-variant/30">
              {APPROVAL_MODES.map((m, i) => (
                <button
                  key={m.value}
                  type="button"
                  role="radio"
                  aria-checked={i === selectedIndex}
                  onClick={() => handleModeChange(m)}
                  className={cn(
                    'flex-1 min-w-0 px-xs py-sm rounded-lg font-label-md text-center cursor-pointer transition-all duration-(--duration-normal)',
                    'focus-visible:outline focus-visible:outline-2 focus-visible:outline-primary',
                    i === selectedIndex
                      ? 'bg-primary text-on-primary font-bold shadow-e1'
                      : 'text-on-surface-variant hover:text-primary hover:bg-surface-container-high',
                  )}
                >
                  <span className="block truncate">{t(m.labelKey)}</span>
                </button>
              ))}
            </div>
            {currentMode.rawLabel != null ? (
              // Out-of-table engine value: say what it is instead of pasting
              // a tier description that doesn't apply.
              <p
                className="font-body-sm text-on-surface-variant mt-sm px-xs"
                data-testid="approval-mode-raw-hint"
              >
                {intl.formatMessage({ id: 'settings.general.approvalMode.rawHint' }, { value: currentMode.rawLabel })}
              </p>
            ) : (
              <p className="font-body-sm text-on-surface-variant mt-sm px-xs">
                {t(currentMode.descriptionKey)}
              </p>
            )}
            {/* GB P2-4: the tier only moves the auto-approve baseline —
                High-risk actions keep their confirmation prompt regardless
                (same note the composer's switcher carries). */}
            <p className="font-body-xs text-on-surface-variant/80 mt-xs px-xs flex items-start gap-xs">
              <span className="material-symbols-outlined icon-sm shrink-0 mt-[2px]" aria-hidden="true">gpp_maybe</span>
              {t('chat.input.mode.highRiskNote')}
            </p>
            {/* Design §7.2: expert modes (readonly / dontAsk / bypass) are
                not in the quick tiers — they live behind this advanced
                picker. Selecting one writes the same approval_mode key. */}
            <div className="mt-md flex items-center gap-sm px-xs">
              <label
                className="font-label-md text-on-surface-variant whitespace-nowrap"
                htmlFor="approval-mode-advanced"
              >
                {t('settings.general.approvalMode.advanced')}
              </label>
              <select
                id="approval-mode-advanced"
                className="flex-1 min-w-0 bg-surface-container-low border border-outline-variant/30 rounded-lg px-sm py-xs font-body-md text-on-surface cursor-pointer"
                value={ADVANCED_MODES.find(m => m.value === currentMode.value)?.value ?? ''}
                onChange={e => {
                  const option = ADVANCED_MODES.find(m => m.value === e.target.value)
                  if (option) handleModeChange(option)
                }}
              >
                <option value="">{t('settings.general.approvalMode.advanced.none')}</option>
                {ADVANCED_MODES.map(m => (
                  <option key={m.value} value={m.value}>{t(m.labelKey)}</option>
                ))}
              </select>
            </div>
          </div>
        </section>

        {/* Language */}
        <section className="bg-surface-container-lowest rounded-xl border border-outline-variant/30 p-xl shadow-e1 transition-all hover:shadow-e2">
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
        <section className="bg-surface-container-lowest rounded-xl border border-outline-variant/30 p-xl shadow-e1 transition-all hover:shadow-e2">
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
        <section className="bg-surface-container-lowest rounded-xl border border-outline-variant/30 p-xl shadow-e1">
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
            {/* 2026-09-29 provider review §3-A1: `config.provider`/`config.model`
                are dead since ADR-0005 (the row read "Not configured" for every
                user). Render the real active provider/model from the
                get_provider_status snapshot; "—" only when genuinely unset. */}
            <div className="flex justify-between items-center py-sm">
              <span className="font-label-md text-on-surface-variant">{t('settings.general.sessionInfo.activeProvider')}</span>
              <span className="font-label-md text-on-surface font-bold">{activeProviderLabel ?? t('settings.general.sessionInfo.notConfigured')}</span>
            </div>
            <div className="flex justify-between items-center py-sm">
              <span className="font-label-md text-on-surface-variant">{t('settings.general.sessionInfo.model')}</span>
              <span className="font-label-md text-on-surface font-bold">{providerStatus?.model ?? t('settings.general.sessionInfo.notConfigured')}</span>
            </div>
            <div className="flex justify-between items-center py-sm">
              <span className="font-label-md text-on-surface-variant">{t('settings.general.sessionInfo.workingDir')}</span>
              <span className="font-label-md text-on-surface font-bold font-mono text-sm truncate max-w-[300px]">{config?.working_dir ?? t('settings.general.sessionInfo.notSet')}</span>
            </div>
          </div>
        </section>

        {/* PM-12: persisted message ratings, aggregated per session */}
        <FeedbackSummaryCard />

        {/* P1-6 — migration wizard entry (import from Claude Code / ZCode) */}
        <section className="bg-surface-container-lowest rounded-xl border border-outline-variant/30 p-xl shadow-e1">
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
        <section className="bg-surface-container-lowest rounded-xl border border-outline-variant/30 p-xl shadow-e1">
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
