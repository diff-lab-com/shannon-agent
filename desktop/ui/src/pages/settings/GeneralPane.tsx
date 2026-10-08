import { useState } from 'react'
import { useT } from '@/i18n'
import * as api from '@/lib/tauri-api'
import { toast } from 'sonner'
import { toastError } from '@/lib/errorToast'
import { Button } from '@/components/ui/button'
import { ConfirmDialog } from '@/components/ui/confirm-dialog'
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
// closes with the 数据 card, in an honest form (audit §3 Wave A-13):
//   - 「导出全部会话数据」: the engine only ships per-session export
//     (export_session, commands_sessions.rs:1301 — the sidebar session
//     menu's 导出 Markdown), no bulk command, so the full-export button is
//     an amber dashed「即将支持」badge instead of a dead control;
//   - 「清除本地缓存」: moved here from the dev-gated 高级 page (the
//     advanced Memory card), same clear_cache confirm flow;
//   - the「会话数据仅存本机 ~/.shannon」note as designed.
// The design's 启动 card is NOT implemented: the engine has no
// restore-last-session-on-launch capability to honestly wire it to.
export default function GeneralPane() {
  const t = useT()
  // Copy of the advanced page's clear-cache flow (AdvancedSettings
  // handleClearCache + ConfirmDialog) — same config key, same dialog copy.
  const [clearing, setClearing] = useState(false)
  const [showClearConfirm, setShowClearConfirm] = useState(false)
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

      {/* 数据 (design 12-settings-general.html:236-243) — honest edition. */}
      <section className="space-y-md" aria-labelledby="general-pane-data-heading">
        <h2 id="general-pane-data-heading" className="text-headline-sm font-medium">
          {t('settings.general.data.title')}
        </h2>
        <div
          className="bg-surface-container-lowest rounded-xl border border-outline-variant/30 p-xl shadow-e1 space-y-md"
          data-testid="general-data-card"
        >
          {/* Bulk export has no engine command yet — honesty beats a dead
              button (README §3.5: 不支持的能力用琥珀虚线徽章明说). */}
          <div>
            <span
              className="inline-flex items-center gap-xs rounded-full border border-dashed border-warning/50 bg-warning-container text-on-warning-container px-sm py-xs font-label-md"
              data-testid="data-export-full-badge"
            >
              <span className="material-symbols-outlined icon-sm" aria-hidden="true">download</span>
              {t('settings.general.data.exportFullSoon')}
            </span>
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
