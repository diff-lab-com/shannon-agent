import { useState } from 'react'
import { useIntl, type PrimitiveType } from 'react-intl'
import { toast } from 'sonner'
import { Spinner } from '@/components/ui/loading-state'
import { cn } from '@/lib/utils'
import * as api from '@/lib/tauri-api'
import { toastError } from '@/lib/errorToast'
import { useCatalog } from '@/context/CatalogContext'
import { APPROVAL_MODES, ADVANCED_MODES, PLAN_OPTION, approvalModeOption } from '@/lib/approvalModes'
import PermissionsSettings from '@/components/settings/PermissionsSettings'

// IA redesign 2026-10 (ADVERSARIAL-REVIEW §2): the design places the default
// execution tier (审批模式) inside 权限与安全. Moving the control out of
// GeneralSettings would break a group of tests and split the composer's
// shared `approval_mode` surface, so instead this pane opened with a guidance
// card pointing at General — which left the two pages pointing at each other
// in a ring (design-parity R2, audit §3.2: 双引导卡环形跳转).
//
// design-parity R2 (Wave B′): the guidance card is retired. This pane now
// carries the REAL control as its first card — 默认执行模式 (design
// 12-settings-security.html:163-173), a four-stop segmented ladder
// ([...APPROVAL_MODES, PLAN_OPTION]) over the SAME `approval_mode` config
// key the composer's pill writes, plus the expert advanced picker that used
// to live on General. The two engine concepts stay SEPARATE by ruling
// (design README §5-5): this card is the approval ladder (approval_mode);
// the rule presets below are the permission tier (严格/平衡/宽松,
// permission_profile) — the distinction note inside the card says so, and
// switching a preset never moves the tier (backend 8803a519d).
export default function PermissionsPane() {
  const intl = useIntl()
  const t = (id: string, values?: Record<string, PrimitiveType>) =>
    values === undefined ? intl.formatMessage({ id }) : intl.formatMessage({ id }, values)
  const { config, refreshConfig } = useCatalog()
  const [saving, setSaving] = useState(false)

  // Same honest read resolver the composer pill uses: in-table values select
  // their segment; out-of-table engine values (confirm / dont_ask / …)
  // select NOTHING and name the raw value (Round-2 review, P1-10).
  const currentMode = approvalModeOption(config?.approval_mode)
  const fourStops = [...APPROVAL_MODES, PLAN_OPTION]
  const selectedIndex = currentMode.rawLabel == null
    ? fourStops.findIndex(m => m.value === currentMode.value)
    : -1

  const handleModeChange = async (option: (typeof fourStops)[number]) => {
    setSaving(true)
    try {
      await api.configure({ key: 'approval_mode', value: option.value })
      await refreshConfig()
      // Safety switch — the read above re-derives from config once the write
      // has actually landed (P1-10: no optimistic update, no stale mirror).
      toast.success(t('settings.general.approvalMode.updated', { label: t(option.labelKey) }))
    } catch (e) { toastError(t('settings.general.approvalMode.updateFailed'), e) }
    setSaving(false)
  }

  return (
    <div className="space-y-3">
      {/* 默认执行模式 (design 12-settings-security.html:163-173) — the
          entity card. Segment styling mirrors the retired GeneralSettings
          control (equal flex segments, short label only; the selected mode's
          description moves to a helper line below). */}
      <section
        className="bg-surface-container-lowest rounded-xl border border-outline-variant/30 p-4 shadow-e1 transition-all hover:shadow-e2"
        data-testid="permissions-execution-mode-card"
      >
        <div className="flex items-center gap-md mb-xs">
          <span className="material-symbols-outlined text-primary" style={{fontVariationSettings: "'FILL' 1"}}>shield</span>
          <h2 className="text-label-lg font-semibold text-on-surface">{t('settings.permissions.executionMode.title')}</h2>
          {saving && <Spinner className="text-primary text-body-lg" />}
        </div>
        <p className="font-body-sm text-on-surface-variant mb-xl">{t('settings.permissions.executionMode.desc')}</p>
        <div role="radiogroup" aria-label={intl.formatMessage({ id: 'settings.general.approvalMode.sliderAria' })}>
          <div className="flex rounded-xl bg-surface-container-low p-xs gap-xs border border-outline-variant/30">
            {fourStops.map((m, i) => (
              <button
                key={m.value}
                type="button"
                role="radio"
                aria-checked={i === selectedIndex}
                data-testid={`permissions-execution-mode-${m.value}`}
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
              not in the quick tiers — they moved here together with the
              entity card. Selecting one writes the same approval_mode key. */}
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
        {/* README §5-5 ruling: the two engine configs are NOT merged. This
            card is the approval ladder (approval_mode); the 规则预设 below
            are the permission tier (permission_profile) — switching a preset
            never changes the execution mode. */}
        <p
          className="font-body-xs text-on-surface-variant/80 mt-md px-xs py-sm rounded-lg bg-surface-container-low border border-outline-variant/30"
          data-testid="permissions-execution-mode-distinction"
        >
          {t('settings.permissions.executionMode.distinction')}
        </p>
      </section>
      <PermissionsSettings />
    </div>
  )
}
