import { useEffect, useState } from 'react'
import { useIntl } from 'react-intl'
import { toast } from 'sonner'
import { useCatalog } from '@/context/CatalogContext'
import * as api from '@/lib/tauri-api'
import { toastError } from '@/lib/errorToast'
import { Switch } from '@/components/ui/switch'
import EffectBadge from './EffectBadge'

/**
 * Settings → 会话 (Settings R3, T6): session lifecycle settings.
 *
 * Two cards:
 * ① 「自动压缩上下文」 — the engine-level `auto_compact_enabled` switch
 *    (persisted as `context_auto_compact`, configure key
 *    `context.auto_compact`). The engine is rebuilt per message, so a flip
 *    applies to the NEXT message — hence the `EffectBadge kind="new-session"`
 *    plus an explicit scope note.
 * ② 「会话存储管理」 — migrated verbatim from AdvancedSettings (卡A GC):
 *    the auto-clean switch + retention gear, with a new 7-day option. The
 *    Advanced page keeps a cross-link (i18n
 *    `settings.advanced.movedToSession`) instead of the card.
 * ③ 「提问自动继续」 (Settings R3 T8) — the `chat_ask_user_auto_continue`
 *    switch: auto-answer an agent question left unanswered for 5 minutes.
 *
 * Later tasks in this batch append more cards to this page (see the anchor
 * comment at the end of the JSX).
 */

export default function SessionSettings() {
  const { config, refreshConfig } = useCatalog()
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })

  // ① Auto-compaction: default ON (backend serde default true), follows the
  // persisted config on refresh like every other config-backed toggle.
  const [autoCompact, setAutoCompact] = useState(config?.context_auto_compact ?? true)
  useEffect(() => {
    setAutoCompact(config?.context_auto_compact ?? true)
  }, [config?.context_auto_compact])

  // ② Session GC (卡A, migrated from Advanced): auto-clean switch (default
  // off) + retention gear (永不 0 / 7 / 30 / 90 days).
  const [sessionGcEnabled, setSessionGcEnabled] = useState(config?.session_gc_enabled ?? false)
  const [sessionRetentionDays, setSessionRetentionDays] = useState<number>(
    config?.session_retention_days ?? 0,
  )
  useEffect(() => {
    setSessionGcEnabled(config?.session_gc_enabled ?? false)
  }, [config?.session_gc_enabled])
  useEffect(() => {
    setSessionRetentionDays(config?.session_retention_days ?? 0)
  }, [config?.session_retention_days])

  // ③ Ask auto-continue (T8): default OFF (backend serde default) — the
  // agent waits for the user's answer indefinitely until opted in.
  const [askAutoContinue, setAskAutoContinue] = useState(config?.chat_ask_user_auto_continue ?? false)
  useEffect(() => {
    setAskAutoContinue(config?.chat_ask_user_auto_continue ?? false)
  }, [config?.chat_ask_user_auto_continue])

  const handleToggle = async (key: string, value: boolean, setter: (v: boolean) => void) => {
    setter(value)
    try {
      await api.configure({ key, value: String(value) })
      await refreshConfig()
      toast.success(intl.formatMessage({ id: 'settings.advanced.toggled' }, { key: key.replace(/[._]/g, ' '), state: value ? t('settings.advanced.enabled') : t('settings.advanced.disabled') }))
    } catch (e) {
      // Don't leave the switch lying — re-read the persisted config so the
      // config→state sync effects put the toggle back where the disk says.
      toastError(t('settings.advanced.updateFailed'), e)
      refreshConfig().catch(() => {})
    }
  }

  // 卡A GC: persist the retention gear. `0` (永不) is written literally —
  // the backend maps it to "never auto-delete" — so the value the user
  // picked is exactly the value stored.
  const handleRetentionChange = async (next: string) => {
    const days = Number(next)
    if (!Number.isInteger(days) || days < 0) return
    setSessionRetentionDays(days)
    try {
      await api.configure({ key: 'session_retention_days', value: String(days) })
      await refreshConfig()
      toast.success(t('settings.advanced.sessionGc.saved'))
    } catch (e) {
      toastError(t('settings.advanced.updateFailed'), e)
    }
  }

  return (
    <div className="pb-xl">
      <p className="font-body-md text-on-surface-variant mb-md">{t('settings.session.title')}</p>
      <div className="space-y-lg">
        {/* ① 自动压缩上下文 — off = full model I/O preserved. The engine is
            rebuilt per message, so the ruling wording is "对下一条消息生效"
            (not "new session"). */}
        <section
          className="bg-surface-container-lowest rounded-xl border border-outline-variant/30 p-xl shadow-e1 transition-all hover:shadow-e2"
          data-testid="session-autocompact-card"
        >
          <div className="flex items-center gap-md mb-xs">
            <span className="material-symbols-outlined text-primary" style={{ fontVariationSettings: "'FILL' 1" }} aria-hidden="true">compress</span>
            <h3 className="font-headline-md text-headline-md">{t('settings.session.compaction.title')}</h3>
            <span className="flex-1" />
            <EffectBadge kind="new-session" />
          </div>
          <p className="font-body-sm text-on-surface-variant mb-md">{t('settings.session.compaction.help')}</p>
          <div className="flex items-center justify-between gap-md">
            <div>
              <div className="font-label-md text-body-sm text-on-surface font-semibold mb-xs">{t('settings.session.compaction.toggle')}</div>
              {/* 裁决: the engine is rebuilt per turn — a flip lands on the
                  next message, which is narrower than "new session". */}
              <div className="font-label-sm text-label-sm text-on-surface-variant leading-tight">{t('settings.session.compaction.scope')}</div>
            </div>
            <Switch
              checked={autoCompact}
              onCheckedChange={v => void handleToggle('context.auto_compact', v, setAutoCompact)}
              className="shrink-0"
              aria-label={t('settings.session.compaction.title')}
              data-testid="session-autocompact-switch"
            />
          </div>
        </section>

        {/* ② 会话存储管理 — the 卡A GC card, migrated verbatim from
            AdvancedSettings (which keeps only a cross-link). The description
            carries the informed-consent copy the review required: 仅清理已
            归档会话；按最后活跃时间计时；默认永不自动删除. */}
        <div className="bg-surface-container-lowest p-lg rounded-xl shadow-e1 border border-outline-variant/30 group hover:shadow-e2 transition-shadow" data-testid="session-gc-card">
          <div className="flex items-center gap-md mb-md">
            <div className="p-sm bg-primary-container rounded-lg text-on-primary-container flex items-center justify-center">
              <span className="material-symbols-outlined">auto_delete</span>
            </div>
            <h3 className="font-headline-md text-headline-md font-bold text-on-surface">{t('settings.advanced.sessionGc.title')}</h3>
          </div>
          <p className="text-on-surface-variant text-body-sm mb-lg">{t('settings.advanced.sessionGc.desc')}</p>
          <div className="flex items-center justify-between gap-md">
            <div>
              <div className="font-label-md text-body-sm text-on-surface font-semibold mb-xs">{t('settings.advanced.sessionGc.enabled')}</div>
              <div className="font-label-sm text-label-sm text-on-surface-variant leading-tight">{t('settings.advanced.sessionGc.enabledDesc')}</div>
            </div>
            <Switch checked={sessionGcEnabled} onCheckedChange={v => void handleToggle('session_gc_enabled', v, setSessionGcEnabled)} className="shrink-0" aria-label={t('settings.advanced.sessionGc.enabled')} />
          </div>
          <div className="mt-md">
            <label className="block font-label-sm text-label-sm text-on-surface-variant mb-xs" htmlFor="session-retention-select">
              {t('settings.advanced.sessionGc.retention')}
            </label>
            {/* Native select (the settings modals' pattern) — the wire value
                is the plain day count, `0` = 永不 (never auto-delete). T6 adds
                the 7-day gear between 永不 and 30. */}
            <select
              id="session-retention-select"
              className="w-full px-md py-sm bg-surface text-on-surface border border-outline-variant/50 rounded-lg outline-none focus:ring-2 focus:ring-primary font-body-sm cursor-pointer"
              value={String(sessionRetentionDays)}
              onChange={e => void handleRetentionChange(e.target.value)}
              aria-label={t('settings.advanced.sessionGc.retention')}
            >
              <option value="0">{t('settings.advanced.sessionGc.retention.never')}</option>
              <option value="7">{t('settings.session.retention.7')}</option>
              <option value="30">{t('settings.advanced.sessionGc.retention.30')}</option>
              <option value="90">{t('settings.advanced.sessionGc.retention.90')}</option>
            </select>
            <p className="font-label-sm text-label-xs text-on-surface-variant mt-xs">{t('settings.advanced.sessionGc.retentionDesc')}</p>
          </div>
        </div>

        {/* ③ 提问自动继续 — Settings R3 T8: auto-answer an agent question
            left unanswered for 5 minutes ("continue on your best judgment").
            The ask_user handler reads the switch live before each question's
            wait, so a flip applies to the NEXT question — instant. */}
        <section
          className="bg-surface-container-lowest rounded-xl border border-outline-variant/30 p-xl shadow-e1 transition-all hover:shadow-e2"
          data-testid="session-ask-auto-continue-card"
        >
          <div className="flex items-center gap-md mb-xs">
            <span className="material-symbols-outlined text-primary" style={{ fontVariationSettings: "'FILL' 1" }} aria-hidden="true">contact_support</span>
            <h3 className="font-headline-md text-headline-md">{t('settings.session.askAutoContinue.title')}</h3>
            <span className="flex-1" />
            <EffectBadge kind="instant" />
          </div>
          <p className="font-body-sm text-on-surface-variant mb-md">{t('settings.session.askAutoContinue.help')}</p>
          <div className="flex items-center justify-between gap-md">
            <div className="font-label-md text-body-sm text-on-surface font-semibold mb-xs">{t('settings.session.askAutoContinue.toggle')}</div>
            <Switch
              checked={askAutoContinue}
              onCheckedChange={v => void handleToggle('chat.ask_user_auto_continue', v, setAskAutoContinue)}
              className="shrink-0"
              aria-label={t('settings.session.askAutoContinue.title')}
              data-testid="session-ask-auto-continue-switch"
            />
          </div>
        </section>

        {/* 后续任务在此追加: 自动归档 / 发送行为 / 分组 */}
      </div>
    </div>
  )
}
