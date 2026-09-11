import { useEffect, useState } from 'react'
import { useIntl } from 'react-intl'
import * as api from '@/lib/tauri-api'
import type { ContextBreakdown } from '@/types'
import { useSessions } from '@/context/SessionContext'
import { useT } from '@/i18n'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'

const CATEGORY_LABEL_KEYS: Record<string, string> = {
  system: 'usage.ctx.system',
  tools: 'usage.ctx.tools',
  skills: 'usage.ctx.skills',
  memory: 'usage.ctx.memory',
  mcp: 'usage.ctx.mcp',
  conversation: 'usage.ctx.conversation',
}

const BAR_COLORS = ['bg-primary', 'bg-secondary', 'bg-tertiary', 'bg-success', 'bg-warning', 'bg-error']

/**
 * Current-session cost panel (audit C13 / competitive-research G3): the
 * Hermes-style "cost as an explainable UI" — six-category context breakdown,
 * window fill, and the session budget cap. All data comes from the existing
 * cost_commands.rs surface; this component is pure presentation.
 */
export default function CurrentSessionCostPanel() {
  const intl = useIntl()
  const t = useT()
  const { currentSessionId } = useSessions()
  const [breakdown, setBreakdown] = useState<ContextBreakdown | null>(null)
  const [budget, setBudget] = useState<string>('')
  const [saving, setSaving] = useState(false)

  useEffect(() => {
    if (!currentSessionId) return
    let alive = true
    api.getSessionContextBreakdown(currentSessionId)
      .then(b => { if (alive) setBreakdown(b) })
      .catch(e => console.warn('context breakdown failed:', e))
    api.getSessionBudget(currentSessionId)
      .then(b => { if (alive && b != null) setBudget(String(b)) })
      .catch(e => console.warn('session budget read failed:', e))
    return () => { alive = false }
  }, [currentSessionId])

  if (!currentSessionId || !breakdown) return null

  const pct = breakdown.contextWindow
    ? Math.min(100, (breakdown.totalTokens / breakdown.contextWindow) * 100)
    : null

  const saveBudget = async () => {
    if (!currentSessionId) return
    setSaving(true)
    try {
      const n = budget.trim() === '' ? null : Number(budget)
      if (n == null || Number.isFinite(n)) await api.setSessionBudget(currentSessionId, n)
    } finally {
      setSaving(false)
    }
  }

  return (
    <section aria-labelledby="usage-current-heading" className="mb-lg glass-surface rounded-2xl p-lg">
      <h2 id="usage-current-heading" className="font-label-lg font-bold text-on-surface mb-md flex items-center gap-xs">
        <span className="material-symbols-outlined text-primary" aria-hidden="true">donut_small</span>
        {t('usage.ctx.heading')}
      </h2>
      <div className="flex items-baseline gap-sm mb-sm">
        <span className="font-headline-md text-[24px] font-bold text-on-surface">
          {intl.formatNumber(breakdown.totalTokens)}
        </span>
        <span className="font-body-sm text-on-surface-variant">
          {breakdown.contextWindow != null
            ? t('usage.ctx.ofWindow', { window: intl.formatNumber(breakdown.contextWindow) })
            : t('usage.ctx.noWindow')}
        </span>
      </div>
      {pct != null && (
        <div className="h-2 rounded-full bg-surface-container-low overflow-hidden mb-md" role="progressbar" aria-valuenow={Math.round(pct)} aria-valuemin={0} aria-valuemax={100}>
          <div className="h-full bg-primary transition-all" style={{ width: `${pct}%` }} />
        </div>
      )}
      <ul className="grid grid-cols-1 md:grid-cols-2 gap-x-lg gap-y-xs mb-md">
        {breakdown.categories.map((c, i) => {
          const share = breakdown.totalTokens === 0 ? 0 : (c.tokens / breakdown.totalTokens) * 100
          return (
            <li key={c.key} className="flex items-center gap-sm">
              <span className={`w-2 h-2 rounded-sm shrink-0 ${BAR_COLORS[i % BAR_COLORS.length]}`} aria-hidden="true" />
              <span className="font-label-sm text-on-surface-variant w-28 shrink-0">
                {t(CATEGORY_LABEL_KEYS[c.key] ?? 'usage.ctx.conversation')}
              </span>
              <div className="flex-1 h-1.5 rounded-full bg-surface-container-low overflow-hidden">
                <div className={`h-full ${BAR_COLORS[i % BAR_COLORS.length]}`} style={{ width: `${share}%` }} />
              </div>
              <span className="font-label-sm text-on-surface-variant w-16 text-right">{intl.formatNumber(c.tokens)}</span>
            </li>
          )
        })}
      </ul>
      <div className="flex items-center gap-sm">
        <label className="font-label-sm text-on-surface-variant flex items-center gap-sm">
          {t('usage.ctx.budgetLabel')}
          <Input
            type="number"
            min={0}
            step="0.5"
            value={budget}
            onChange={e => setBudget(e.target.value)}
            placeholder={t('usage.ctx.budgetPlaceholder')}
            className="w-28"
          />
        </label>
        <Button
          variant="outline"
          onClick={() => void saveBudget()}
          disabled={saving}
          className="cursor-pointer"
        >
          {t('usage.ctx.budgetSave')}
        </Button>
      </div>
    </section>
  )
}
