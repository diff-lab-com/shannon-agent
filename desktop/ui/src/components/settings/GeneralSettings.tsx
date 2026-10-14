import { useEffect, useId, useState } from 'react'
import { open as openDialog } from '@tauri-apps/plugin-dialog'
import { useNavigate } from 'react-router-dom'
import { toast } from 'sonner'
import { useIntl } from 'react-intl'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { cn } from '@/lib/utils'
import { useCatalog } from '@/context/CatalogContext'
import { useI18n, SUPPORTED_LOCALES, type LocalePref } from '@/i18n'
import * as api from '@/lib/tauri-api'
import { toastError } from '@/lib/errorToast'
import { readDensityPref, setDensityPref, type DensityPref } from '@/lib/density'
import { readShowThinkingPref, setShowThinkingPref, type ShowThinkingPref } from '@/lib/thinkingPref'
import { getLinkTarget, setLinkTarget as setLinkTargetPref, type LinkTarget } from '@/lib/openLink'
import { Switch } from '@/components/ui/switch'
import { useArtifact } from '@/components/artifact/ArtifactContext'
import { approvalModeOption } from '@/lib/approvalModes'
import { WELCOME_SEEN_KEY } from '@/pages/Welcome'
import MigrationWizard from '@/components/migration/MigrationWizard'
import PersonaPackSettings from './PersonaPackSettings'
import { FeedbackSummaryCard } from './FeedbackSummaryCard'
import EffectBadge from './EffectBadge'

// GB P2-4: the tiers come from the SHARED table (lib/approvalModes) — the
// same values, labels and descriptions the composer's quick switcher
// renders, over the same `approval_mode` config key. Round-2: the READ also
// goes through the shared resolver (approvalModeOption), so out-of-table
// engine values like the factory default "confirm" show the raw value with
// no tier selected instead of masquerading as a pickable tier.
//
// design-parity R2 (Wave B′, audit §3.2): the CONTROL itself moved to the
// 权限与安全 pane (PermissionsPane's 默认执行模式 entity card, design
// 12-settings-security.html:163-173) — killing the two-guidance-card ring
// jump. What stays here is a short link card: the current tier readout
// (same honest resolver) plus a jump to where the tier is configured.

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
  // Settings R3 T9 — 显示思考过程 three-tier display pref. Purely
  // front-end localStorage ('shannon.chat.showThinking'): ChatMessage /
  // StreamingResponse read it on every render, so a pick here applies to
  // the message flow instantly with no backend round-trip.
  const [showThinking, setShowThinkingState] = useState<ShowThinkingPref>(readShowThinkingPref)
  const handleShowThinkingChange = (next: ShowThinkingPref) => {
    setShowThinkingState(next)
    setShowThinkingPref(next)
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
  const { localePref, setLocale } = useI18n()
  // P1-6 — migration wizard (import from Claude Code / ZCode).
  const [migrationOpen, setMigrationOpen] = useState(false)
  // Settings R3 T3 — platform + keep-awake capability probe. null = the
  // probe hasn't answered yet (or failed): the hw-accel card stays hidden
  // until the platform is known (so macOS never sees a flash) and the
  // prevent-sleep switches stay enabled (the backend defaults are safe).
  const [powerCaps, setPowerCaps] = useState<api.PowerCapabilities | null>(null)

  // Settings R3 T3 — probe once on mount.
  useEffect(() => {
    let cancelled = false
    api.getPowerCapabilities()
      .then(caps => { if (!cancelled) setPowerCaps(caps) })
      .catch(() => {
        // Backend missing (web build / old binary): assume supported so the
        // switches stay usable; the backend defaults are safe.
        if (!cancelled) setPowerCaps({ platform: 'unknown', keepAwakeSupported: true })
      })
    return () => { cancelled = true }
  }, [])

  const handleRerunWizard = () => {
    window.localStorage.removeItem(WELCOME_SEEN_KEY)
    navigate('/welcome')
  }

  const handleLocaleChange = (next: LocalePref) => {
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
  // values select NOTHING and name the raw engine value. (R2: the radio
  // group itself lives in PermissionsPane; this card only reads.)
  const currentMode = approvalModeOption(config?.approval_mode)

  // design-parity R2 (M3a, design 12-settings-general.html:212-217): the
  // session-defaults' 默认工作目录 became WRITABLE. Type an absolute path
  // or pick one with the OS directory picker (the same plugin-dialog `open`
  // the Welcome card uses — setup.ts mocks it); the write goes through
  // configure('working_dir'), the exact key the engine resolves sessions
  // from (commands_config.rs "working_dir" arm). Before writing, the path
  // is probed with get_file_tree — its "Path is not a directory" error is
  // the only directory-existence probe on the IPC surface (path_exists is
  // is_file()-only AND scope-gated, so it can't answer for arbitrary home
  // subdirectories); any rejection renders as the inline 目录不存在 hint
  // and nothing is persisted.
  const [workingDirDraft, setWorkingDirDraft] = useState(config?.working_dir ?? '')
  useEffect(() => {
    setWorkingDirDraft(config?.working_dir ?? '')
  }, [config?.working_dir])
  const [workingDirError, setWorkingDirError] = useState<string | null>(null)
  const [workingDirBusy, setWorkingDirBusy] = useState(false)
  const workingDirErrorId = useId()

  const browseWorkingDir = async () => {
    try {
      const sel = await openDialog({ directory: true, multiple: false })
      if (typeof sel === 'string') setWorkingDirDraft(sel)
    } catch { /* user cancelled / picker unavailable — keep the draft */ }
  }

  const saveWorkingDir = async () => {
    const dir = workingDirDraft.trim()
    if (dir === '' || workingDirBusy) return
    setWorkingDirBusy(true)
    setWorkingDirError(null)
    try {
      await api.getFileTree(dir)
    } catch {
      // Not-a-directory / missing / inaccessible — inline error, no write.
      setWorkingDirError(t('settings.general.sessionInfo.workingDirMissing'))
      setWorkingDirBusy(false)
      return
    }
    try {
      await api.configure({ key: 'working_dir', value: dir })
      await refreshConfig()
      toast.success(intl.formatMessage({ id: 'settings.general.sessionInfo.workingDirSaved' }))
    } catch (e) {
      toastError(t('settings.general.sessionInfo.workingDirFailed'), e)
    }
    setWorkingDirBusy(false)
  }

  // Settings R3 T3 — write one of the System-group switches. All three are
  // plain configure writes: `hardware_acceleration` takes effect on the
  // next launch (the backend injects the disable-GPU env vars before the
  // webview exists), the two power switches apply immediately on the
  // backend (keep-awake starts/stops the wake lock; the run-time blocker
  // is re-read at the start of every run).
  const handleSystemToggle = async (key: string, value: boolean) => {
    try {
      await api.configure({ key, value: String(value) })
      await refreshConfig()
    } catch (e) { toastError(t('settings.system.updateFailed'), e) }
  }

  // D5 方案① — 主动任务推荐 presentation toggle. A pure configure write:
  // the backend never gates anything on this key — the UI reads it live
  // (completion chips + welcome refresh/filter), so a flip applies
  // immediately with no restart.
  const handleSuggestionsToggle = async (value: boolean) => {
    try {
      await api.configure({ key: 'suggestions.enabled', value: String(value) })
      await refreshConfig()
    } catch (e) { toastError(t('settings.general.suggestions.updateFailed'), e) }
  }

  return (
    <div className="max-w-narrow">
      <p className="font-body-md text-on-surface-variant mb-md">{t('settings.general.subheader')}</p>

      <div className="space-y-lg">
        {/* Autonomy Level — short link card since design-parity R2: the
            默认执行模式 entity card (four-stop ladder + expert picker,
            same approval_mode key as the composer) LIVES in 权限与安全
            (PermissionsPane). One-liner keeps the honest raw-value readout;
            the button jumps straight there. */}
        <section
          className="bg-surface-container-lowest rounded-xl border border-outline-variant/30 p-xl shadow-e1 transition-all hover:shadow-e2"
          data-testid="approval-mode-link-card"
        >
          <div className="flex items-center gap-md mb-xs">
            <span className="material-symbols-outlined text-primary" style={{fontVariationSettings: "'FILL' 1"}}>auto_awesome</span>
            {/* R3 wording: title aligns with the Permissions pane's 实体卡 — one
                config, one name (「默认执行模式」); 审批模式 stays the concept
                term in the description below. */}
            <h3 className="font-headline-md text-headline-md">{t('settings.permissions.executionMode.title')}</h3>
          </div>
          <p className="font-body-sm text-on-surface-variant mb-xl">
            {intl.formatMessage({ id: 'settings.general.approvalMode.managedInPermissions' }, {
              label: currentMode.rawLabel ?? t(currentMode.labelKey),
            })}
          </p>
          <Button
            variant="outline"
            onClick={() => navigate('/settings/permissions')}
            data-testid="general-to-permissions-link"
          >
            {t('settings.general.approvalMode.managedInPermissionsAction')}
            <span className="material-symbols-outlined icon-sm" aria-hidden="true">arrow_forward</span>
          </Button>
        </section>

        {/* Language */}
        <section className="bg-surface-container-lowest rounded-xl border border-outline-variant/30 p-xl shadow-e1 transition-all hover:shadow-e2">
          <div className="flex items-center gap-md mb-xs">
            <span className="material-symbols-outlined text-primary" style={{fontVariationSettings: "'FILL' 1"}}>translate</span>
            <h3 className="font-headline-md text-headline-md">{intl.formatMessage({ id: 'settings.language.label' })}</h3>
          </div>
          <p className="font-body-sm text-on-surface-variant mb-xl">{intl.formatMessage({ id: 'settings.language.help' })}</p>
          {/* Task 2 (settings-parity R3): the 10-button wall became a select
              styled after the link-target picker below. First option is the
              explicit "follow system" pref — persisted verbatim as 'system'
              so the choice survives restarts; the provider re-probes the OS
              language live whenever it resolves. Language applies instantly,
              so no EffectBadge (brief T2). */}
          <select
            data-testid="settings-language-select"
            value={localePref}
            onChange={e => handleLocaleChange(e.target.value as LocalePref)}
            aria-label={intl.formatMessage({ id: 'settings.language.label' })}
            className="font-label-md text-on-surface bg-surface-container rounded-lg px-sm py-xs border border-outline-variant/30 cursor-pointer"
          >
            <option value="system">{intl.formatMessage({ id: 'settings.language.system' })}</option>
            {SUPPORTED_LOCALES.map(opt => (
              <option key={opt.id} value={opt.id}>{intl.formatMessage({ id: opt.labelKey })}</option>
            ))}
          </select>
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

        {/* Settings R3 T9 — show thinking: three-tier display pref for the
            model's reasoning blocks (history bubbles + live stream). Placed
            next to the other display cards (language / density); segmented
            control mirrors the approval-mode one. */}
        <section className="bg-surface-container-lowest rounded-xl border border-outline-variant/30 p-xl shadow-e1 transition-all hover:shadow-e2">
          <div className="flex items-center gap-md mb-xs">
            <span className="material-symbols-outlined text-primary" style={{fontVariationSettings: "'FILL' 1"}}>psychology</span>
            <h3 className="font-headline-md text-headline-md">{t('settings.general.showThinking.title')}</h3>
            <span className="flex-1" />
            <EffectBadge kind="instant" />
          </div>
          <p className="font-body-sm text-on-surface-variant mb-xl">{t('settings.general.showThinking.help')}</p>
          <div role="radiogroup" aria-label={t('settings.general.showThinking.title')} data-testid="settings-show-thinking-group">
            <div className="flex rounded-xl bg-surface-container-low p-xs gap-xs border border-outline-variant/30">
              {([
                { id: 'all' as const, labelKey: 'settings.general.showThinking.all' },
                { id: 'first' as const, labelKey: 'settings.general.showThinking.first' },
                { id: 'none' as const, labelKey: 'settings.general.showThinking.none' },
              ]).map(opt => (
                <button
                  key={opt.id}
                  type="button"
                  role="radio"
                  aria-checked={showThinking === opt.id}
                  data-testid={`settings-show-thinking-${opt.id}`}
                  onClick={() => handleShowThinkingChange(opt.id)}
                  className={cn(
                    'flex-1 min-w-0 px-xs py-sm rounded-lg font-label-md text-center cursor-pointer transition-all duration-(--duration-normal)',
                    'focus-visible:outline focus-visible:outline-2 focus-visible:outline-primary',
                    showThinking === opt.id
                      ? 'bg-primary text-on-primary font-bold shadow-e1'
                      : 'text-on-surface-variant hover:text-primary hover:bg-surface-container-high',
                  )}
                >
                  <span className="block truncate">{t(opt.labelKey)}</span>
                </button>
              ))}
            </div>
          </div>
        </section>

        {/* D5 方案① — 主动任务推荐: the post-completion action chips and
            the welcome card's 「换一批」 refresh + workspace-aware example
            filtering. Purely a presentation switch — static local rules,
            zero model calls either way; a flip applies immediately. */}
        <section className="bg-surface-container-lowest rounded-xl border border-outline-variant/30 p-xl shadow-e1 transition-all hover:shadow-e2">
          <div className="flex items-center gap-md mb-xs">
            <span className="material-symbols-outlined text-primary" style={{fontVariationSettings: "'FILL' 1"}}>recommend</span>
            <h3 className="font-headline-md text-headline-md">{t('settings.general.suggestions.title')}</h3>
            <span className="flex-1" />
            <EffectBadge kind="instant" />
          </div>
          <p className="font-body-sm text-on-surface-variant mb-xl">{t('settings.general.suggestions.help')}</p>
          <div className="flex justify-between items-center py-sm gap-md">
            <span className="min-w-0">
              <span className="font-label-md text-on-surface block">{t('settings.general.suggestions.toggle')}</span>
              <span className="font-label-sm text-on-surface-variant block">{t('settings.general.suggestions.toggleDesc')}</span>
            </span>
            <Switch
              checked={config?.suggestions_enabled !== false}
              onCheckedChange={v => void handleSuggestionsToggle(v)}
              aria-label={t('settings.general.suggestions.title')}
              data-testid="settings-suggestions-switch"
            />
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
            {/* M3a — the row became a WRITABLE field (input + Browse +
                Save). Save is disabled until the draft drifts; validation
                and error rendering live in saveWorkingDir above. */}
            <div className="py-sm">
              <div className="flex justify-between items-center gap-md">
                <span className="font-label-md text-on-surface-variant">{t('settings.general.sessionInfo.workingDir')}</span>
              </div>
              <div className="flex flex-wrap items-center gap-sm mt-xs">
                <Input
                  value={workingDirDraft}
                  onChange={e => { setWorkingDirDraft(e.target.value); setWorkingDirError(null) }}
                  placeholder={t('settings.general.sessionInfo.notSet')}
                  aria-label={t('settings.general.sessionInfo.workingDir')}
                  aria-invalid={workingDirError != null || undefined}
                  aria-describedby={workingDirError != null ? workingDirErrorId : undefined}
                  className="font-mono text-sm max-w-[320px]"
                  data-testid="settings-working-dir-input"
                />
                <Button
                  variant="outline"
                  size="sm"
                  onClick={() => void browseWorkingDir()}
                  data-testid="settings-working-dir-browse"
                >
                  {t('settings.general.sessionInfo.workingDirBrowse')}
                </Button>
                <Button
                  size="sm"
                  disabled={workingDirBusy || workingDirDraft.trim() === '' || workingDirDraft.trim() === (config?.working_dir ?? '')}
                  onClick={() => void saveWorkingDir()}
                  data-testid="settings-working-dir-save"
                >
                  {t('settings.general.sessionInfo.workingDirSave')}
                </Button>
              </div>
              {workingDirError != null && (
                <p
                  id={workingDirErrorId}
                  role="alert"
                  className="font-label-sm text-label-sm text-error mt-xs"
                  data-testid="settings-working-dir-error"
                >
                  {workingDirError}
                </p>
              )}
              <p className="font-label-xs text-xs text-on-surface-variant/80 mt-xs">
                {t('settings.general.sessionInfo.workingDirHint')}
              </p>
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

        {/* Settings R3 T3 — System group: hardware acceleration + prevent
            sleep. Rendered before the re-run wizard card so the power
            switches (a daily concern) stay above the recovery affordance. */}
        <div className="space-y-xs" data-testid="settings-system-group">
          <h2 className="font-label-md text-on-surface-variant px-xs pt-sm">
            {t('settings.system.title')}
          </h2>

          {/* ① Hardware acceleration — hidden on macOS (WKWebView has no
              supported escape hatch). `platform` comes from the backend
              probe; while it is pending the card stays hidden so macOS
              users never see a flash of a useless card. */}
          {powerCaps && powerCaps.platform !== 'macos' && (
            <section className="bg-surface-container-lowest rounded-xl border border-outline-variant/30 p-xl shadow-e1 transition-all hover:shadow-e2">
              <div className="flex items-center gap-md mb-xs">
                <span className="material-symbols-outlined text-primary" style={{fontVariationSettings: "'FILL' 1"}}>developer_board</span>
                <h3 className="font-headline-md text-headline-md">{t('settings.system.hwAccel.title')}</h3>
                <span className="flex-1" />
                <EffectBadge kind="restart-app" />
              </div>
              <p className="font-body-sm text-on-surface-variant mb-xl">{t('settings.system.hwAccel.help')}</p>
              <div className="flex justify-between items-center py-sm gap-md">
                <span className="font-label-md text-on-surface">{t('settings.system.hwAccel.toggle')}</span>
                <Switch
                  checked={config?.hardware_acceleration !== false}
                  onCheckedChange={v => handleSystemToggle('hardware_acceleration', v)}
                  aria-label={t('settings.system.hwAccel.title')}
                  data-testid="settings-hwaccel-switch"
                />
              </div>
            </section>
          )}

          {/* ② Prevent sleep — task-run blocker (default on) + always-on
              wake lock (default off). Disabled with a note when the probe
              reports no backend (Linux without systemd-inhibit). */}
          <section className="bg-surface-container-lowest rounded-xl border border-outline-variant/30 p-xl shadow-e1 transition-all hover:shadow-e2">
            <div className="flex items-center gap-md mb-xs">
              <span className="material-symbols-outlined text-primary" style={{fontVariationSettings: "'FILL' 1"}}>bedtime</span>
              <h3 className="font-headline-md text-headline-md">{t('settings.system.keepAwake.title')}</h3>
              <span className="flex-1" />
              <EffectBadge kind="instant" />
            </div>
            <p className="font-body-sm text-on-surface-variant mb-xl">{t('settings.system.keepAwake.taskHelp')}</p>
            {powerCaps && !powerCaps.keepAwakeSupported && (
              <p
                className="font-body-sm text-on-surface-variant mb-md px-xs py-sm rounded-lg bg-surface-container-low border border-outline-variant/30"
                data-testid="settings-keepawake-unsupported"
              >
                {t('settings.system.keepAwake.unsupported')}
              </p>
            )}
            <div className="space-y-sm">
              <div className="flex justify-between items-center py-sm gap-md">
                <span className="min-w-0">
                  <span className="font-label-md text-on-surface block">{t('settings.system.keepAwake.taskToggle')}</span>
                  <span className="font-label-sm text-on-surface-variant block">{t('settings.system.keepAwake.taskHelp')}</span>
                </span>
                <Switch
                  checked={config?.power_block_sleep_during_tasks !== false}
                  disabled={powerCaps != null && !powerCaps.keepAwakeSupported}
                  onCheckedChange={v => handleSystemToggle('power.block_sleep_during_tasks', v)}
                  aria-label={t('settings.system.keepAwake.taskToggle')}
                  data-testid="settings-keepawake-task-switch"
                />
              </div>
              <div className="flex justify-between items-center py-sm gap-md">
                <span className="min-w-0">
                  <span className="font-label-md text-on-surface block">{t('settings.system.keepAwake.alwaysToggle')}</span>
                  <span className="font-label-sm text-on-surface-variant block">{t('settings.system.keepAwake.alwaysHelp')}</span>
                </span>
                <Switch
                  checked={config?.power_keep_awake === true}
                  disabled={powerCaps != null && !powerCaps.keepAwakeSupported}
                  onCheckedChange={v => handleSystemToggle('power.keep_awake', v)}
                  aria-label={t('settings.system.keepAwake.alwaysToggle')}
                  data-testid="settings-keepawake-always-switch"
                />
              </div>
            </div>
          </section>
        </div>

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
