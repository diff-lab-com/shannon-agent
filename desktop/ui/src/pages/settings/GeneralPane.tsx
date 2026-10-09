import { useEffect, useState } from 'react'
import { useT } from '@/i18n'
import { save as saveDialog } from '@tauri-apps/plugin-dialog'
import * as api from '@/lib/tauri-api'
import { toast } from 'sonner'
import { toastError } from '@/lib/errorToast'
import { Button } from '@/components/ui/button'
import { ConfirmDialog } from '@/components/ui/confirm-dialog'
import { Switch } from '@/components/ui/switch'
import EffectBadge from '@/components/settings/EffectBadge'
import { useCatalog } from '@/context/CatalogContext'
import GeneralSettings from '@/components/settings/GeneralSettings'
import SessionSettings from '@/components/settings/SessionSettings'

// IA redesign 2026-10 (ADVERSARIAL-REVIEW §2): 会话 content was too thin to
// carry its own section — it is "general preferences" by mental model. The
// 通用 pane therefore stacks the two original components unchanged (the rail
// heading 通用 covers the whole pane; group headings reuse the existing
// `nav.*` keys), General on top, session defaults below. The old
// /settings/session deep link redirects here (App.tsx) — nothing is lost.
//
// Design 12-settings-general.html:236-243 (parity R1 2026-10-08): the pane
// closes with the 数据 card:
//   - 「导出全部会话」: a REAL button since 缓期批 2 — the engine shipped
//     `export_all_sessions` (zip of the sessions container + manifest), so
//     the IA redesign's amber「即将支持」honest placeholder (audit §3
//     Wave A-13) retired in its favor. Destination via the same native save
//     dialog the diagnostics export uses; failures (e.g. missing parent
//     dir) surface verbatim in the error toast;
//   - 「清除本地缓存」: moved here from the dev-gated 高级 page (the
//     advanced Memory card), same clear_cache confirm flow;
//   - the「会话数据仅存本机 ~/.shannon」note as designed.
// 批 1 (2026-10-08): the design's 启动 card NOW exists too — the engine
// always had the restore capability (P1-1), it just had no switch. The card
// sits between the session group and 数据, carrying the two launch-behavior
// toggles (session-window restore + check-only update check), both default
// ON and read once at launch → the EffectBadge is restart-app.
export default function GeneralPane() {
  const t = useT()
  const { config, refreshConfig } = useCatalog()
  // Copy of the advanced page's clear-cache flow (AdvancedSettings
  // handleClearCache + ConfirmDialog) — same config key, same dialog copy.
  const [clearing, setClearing] = useState(false)
  const [showClearConfirm, setShowClearConfirm] = useState(false)
  // 启动 card — both toggles follow the SessionSettings config-backed
  // pattern: local state synced from the refreshed config, optimistic set,
  // revert on failure (the config→state effect puts the switch back where
  // the disk says). Both are read once at launch by the backend/UI, so a
  // flip lands on the NEXT launch — hence the restart-app badge.
  const [restoreSessions, setRestoreSessions] = useState(config?.restore_session_windows_on_launch ?? true)
  const [checkUpdates, setCheckUpdates] = useState(config?.update_check_at_launch ?? true)
  useEffect(() => {
    setRestoreSessions(config?.restore_session_windows_on_launch ?? true)
  }, [config?.restore_session_windows_on_launch])
  useEffect(() => {
    setCheckUpdates(config?.update_check_at_launch ?? true)
  }, [config?.update_check_at_launch])

  const handleLaunchToggle = async (
    key: 'restore_session_windows_on_launch' | 'update_check_at_launch',
    value: boolean,
    setter: (v: boolean) => void,
  ) => {
    setter(value)
    try {
      await api.configure({ key, value: String(value) })
      await refreshConfig()
    } catch (e) {
      toastError(t('settings.advanced.updateFailed'), e)
      refreshConfig().catch(() => {})
    }
  }

  const handleClearCache = async () => {
    setClearing(true)
    try {
      await api.configure({ key: 'clear_cache', value: 'true' })
      toast.success(t('settings.advanced.cacheCleared'))
    } catch (e) {
      toastError(t('settings.advanced.clearCacheFailed'), e)
    }
    setClearing(false)
  }

  // 缓期批 2: 全量会话导出 — same dest-picker flow as the diagnostics
  // export (native save dialog, zip filter, date-stamped default name).
  // The backend overwrites an existing dest and requires the parent dir to
  // exist; any rejection surfaces verbatim in the error toast.
  const [exportingAll, setExportingAll] = useState(false)
  const handleExportAllSessions = async () => {
    let target: string | null = null
    try {
      target = await saveDialog({
        defaultPath: `shannon-sessions-${new Date().toISOString().slice(0, 10)}.zip`,
        filters: [{ name: 'Zip', extensions: ['zip'] }],
      })
    } catch (e) {
      toastError(t('settings.general.data.exportAllSessionsFailed'), e)
      return
    }
    if (!target) return // user cancelled
    setExportingAll(true)
    try {
      const result = await api.exportAllSessions(target)
      toast.success(
        t('settings.general.data.exportAllSessionsDone', {
          count: result.session_count,
          path: result.path,
        }),
      )
    } catch (e) {
      toastError(t('settings.general.data.exportAllSessionsFailed'), e)
    }
    setExportingAll(false)
  }
  return (
    <div className="space-y-xl">
      <section className="space-y-md" aria-labelledby="general-pane-general-heading">
        <h2 id="general-pane-general-heading" className="text-headline-sm font-medium">
          {t('nav.general')}
        </h2>
        <GeneralSettings />
      </section>
      <section className="space-y-md" aria-labelledby="general-pane-session-heading">
        <h2 id="general-pane-session-heading" className="text-headline-sm font-medium">
          {t('nav.session')}
        </h2>
        <SessionSettings />
      </section>

      {/* 启动 (design 12-settings-general 启动 card, 批 1) — the two
          launch-behavior switches. Both default ON (no behavior change for
          existing users) and are read ONCE at launch, so a flip lands on
          the next start (restart-app badge, same semantics the hw-accel
          card carries). Update checking is notify-only by contract: the
          helper text spells out that nothing is ever installed
          automatically. */}
      <section className="space-y-md" aria-labelledby="general-pane-launch-heading">
        <h2 id="general-pane-launch-heading" className="text-headline-sm font-medium">
          {t('settings.general.launch.title')}
        </h2>
        <div
          className="bg-surface-container-lowest rounded-xl border border-outline-variant/30 p-xl shadow-e1 space-y-md"
          data-testid="general-launch-card"
        >
          <div className="flex justify-between items-center gap-md">
            <span className="material-symbols-outlined text-primary shrink-0" style={{ fontVariationSettings: "'FILL' 1" }} aria-hidden="true">rocket_launch</span>
            <EffectBadge kind="restart-app" />
          </div>
          <div className="flex justify-between items-center py-sm gap-md">
            <span className="min-w-0">
              <span className="font-label-md text-on-surface block">{t('settings.general.launch.restoreSessions')}</span>
              <span className="font-label-sm text-on-surface-variant block">{t('settings.general.launch.restoreSessionsDesc')}</span>
            </span>
            <Switch
              checked={restoreSessions}
              onCheckedChange={v => void handleLaunchToggle('restore_session_windows_on_launch', v, setRestoreSessions)}
              aria-label={t('settings.general.launch.restoreSessions')}
              className="shrink-0"
              data-testid="general-launch-restore-switch"
            />
          </div>
          <div className="flex justify-between items-center py-sm gap-md">
            <span className="min-w-0">
              <span className="font-label-md text-on-surface block">{t('settings.general.launch.checkUpdates')}</span>
              <span className="font-label-sm text-on-surface-variant block">{t('settings.general.launch.checkUpdatesDesc')}</span>
            </span>
            <Switch
              checked={checkUpdates}
              onCheckedChange={v => void handleLaunchToggle('update_check_at_launch', v, setCheckUpdates)}
              aria-label={t('settings.general.launch.checkUpdates')}
              className="shrink-0"
              data-testid="general-launch-update-check-switch"
            />
          </div>
        </div>
      </section>

      {/* 数据 (design 12-settings-general.html:236-243) — honest edition. */}
      <section className="space-y-md" aria-labelledby="general-pane-data-heading">
        <h2 id="general-pane-data-heading" className="text-headline-sm font-medium">
          {t('settings.general.data.title')}
        </h2>
        <div
          className="bg-surface-container-lowest rounded-xl border border-outline-variant/30 p-xl shadow-e1 space-y-md"
          data-testid="general-data-card"
        >
          {/* 缓期批 2: the engine ships export_all_sessions — the IA
              redesign's honest「即将支持」placeholder became a real
              button. Still no cloud anything: the zip is written locally
              to the path the user picks. */}
          <div>
            <Button
              variant="outline"
              size="sm"
              disabled={exportingAll}
              onClick={() => void handleExportAllSessions()}
              data-testid="export-all-sessions"
            >
              <span className="material-symbols-outlined icon-sm" aria-hidden="true">download</span>
              {exportingAll
                ? t('settings.general.data.exportAllSessionsWorking')
                : t('settings.general.data.exportAllSessions')}
            </Button>
            <p className="text-body-sm text-on-surface-variant mt-sm max-w-prose">
              {t('settings.general.data.exportSingleHint')}
            </p>
          </div>
          <div className="flex flex-wrap items-center gap-md">
            <Button
              variant="outline"
              size="sm"
              onClick={() => setShowClearConfirm(true)}
              disabled={clearing}
              data-testid="data-clear-cache-button"
            >
              {t('settings.advanced.clearSessionCache')}
            </Button>
            <p className="text-label-md text-on-surface-variant flex items-center gap-xs">
              <span className="material-symbols-outlined icon-sm shrink-0" aria-hidden="true">database</span>
              {t('settings.general.data.localNote')}
            </p>
          </div>
        </div>
      </section>

      <ConfirmDialog
        open={showClearConfirm}
        title={t('settings.advanced.clearSessionCache')}
        message={t('settings.advanced.clearDesc')}
        confirmLabel={t('settings.advanced.clearCache')}
        busyLabel={t('settings.advanced.clearing')}
        cancelLabel={t('settings.advanced.cancel')}
        busy={clearing}
        onConfirm={() => void handleClearCache()}
        onCancel={() => setShowClearConfirm(false)}
      />
    </div>
  )
}
