// SessionUsageDialog — composer 侧的会话用量弹框(2026-09 三项 UX 修复 #3)。
// 模型 chip 旁一次点击即可看到"本会话"的 token 去向,无需绕道用量页。
// 纯组合:六类 breakdown 由 ContextBreakdownCard 自取数(usageTick 驱动
// 流式刷新),预算读取/写入复用 useSessionBudget + BudgetDialog。

import { useState } from 'react'
import { useNavigate } from 'react-router-dom'
import { useIntl } from 'react-intl'
import { Modal, ModalBody, ModalFooter } from '@/components/ui/modal'
import { Button } from '@/components/ui/button'
import { useT } from '@/i18n'
import { useSessions } from '@/context/SessionContext'
import { useSessionBudget } from '@/hooks/useSessionBudget'
import ContextBreakdownCard from '@/components/chat/ContextBreakdownCard'
import BudgetDialog from '@/components/chat/BudgetDialog'

export interface SessionUsageDialogProps {
  open: boolean
  onClose: () => void
  /** 最近一次流式 Usage payload — 变化时 breakdown 重新估算。 */
  usageTick?: unknown
}

/** Compact token formatting for the peak row (96K / 9.6万). The dialog's
 *  own locale (intl.locale) — unlike the rail chip, which reads the runtime
 *  default because it has no intl — so every number in this dialog follows
 *  one locale. */
function fmtCompactTokens(n: number, locale: string): string {
  try {
    return new Intl.NumberFormat(locale, { notation: 'compact', maximumFractionDigits: 1 }).format(n)
  } catch {
    return n.toLocaleString()
  }
}

export default function SessionUsageDialog({ open, onClose, usageTick }: SessionUsageDialogProps) {
  const t = useT()
  const intl = useIntl()
  const navigate = useNavigate()
  const { currentSessionId, sessions } = useSessions()
  const { budget, usage: sessionUsage, refresh: refreshBudget } = useSessionBudget(currentSessionId)
  const [budgetOpen, setBudgetOpen] = useState(false)

  if (!open) return null

  const spent = sessionUsage?.cost_usd ?? 0
  const hasCap = budget != null && budget > 0
  // B4 P2-11: USD via Intl (same approach as SlashResultCard) — the summary
  // line used to hardcode `$x / $y` regardless of locale. The spend keeps
  // 4 fraction digits (the old toFixed(4) precision); the cap falls back to
  // the currency default (2).
  const usd = (v: number) =>
    new Intl.NumberFormat(intl.locale, { style: 'currency', currency: 'USD', maximumFractionDigits: 4 }).format(v)

  // 设计稿 02c S3 (W8): the window-peak row reads SessionInfo
  // .max_context_tokens — the same L0 session-index peak the rail's chip
  // shows. Hidden entirely when the backend reports nothing (数字缺席即隐藏,
  // honesty contract).
  // T2 (R3 §4.1 峰值已用): when the session index ALSO carries the real
  // used numerator (peak_context_used_tokens, the running max of
  // input + cache_read + output per turn), the row renders the % form
  // 「峰值 X / 预算 Y（Z%）」; with only the window it keeps the absolute
  // form; with neither it hides. A used figure without a window budget can
  // never fabricate a percentage — it degrades to the hidden row too.
  const currentSession = currentSessionId != null
    ? sessions?.find(s => s.id === currentSessionId)
    : undefined
  const sessionPeak = currentSession?.max_context_tokens ?? null
  const peakUsed = currentSession?.peak_context_used_tokens ?? null
  const peakLabel = sessionPeak != null && sessionPeak > 0
    ? (peakUsed != null && peakUsed > 0
        ? t('chat.input.usage.sessionPeak.pct', {
            used: fmtCompactTokens(peakUsed, intl.locale),
            budget: fmtCompactTokens(sessionPeak, intl.locale),
            pct: Math.min(100, Math.round((peakUsed / sessionPeak) * 100)),
          })
        : fmtCompactTokens(sessionPeak, intl.locale))
    : null

  return (
    <>
      <Modal open onClose={onClose} title={t('chat.input.usage.title')} size="md">
        <ModalBody className="pt-0 space-y-md">
          {currentSessionId ? (
            <>
              <ContextBreakdownCard sessionId={currentSessionId} usageTick={usageTick} />
              {/* 摘要区(设计稿 02c S3):会话峰值行 + 预算摘要行 — 编辑走
                  既有 BudgetDialog,与右侧 Dock 同源。预算行口径为
                  「花费 / 预算余」(余 = 上限 − 已花,可超支为负);无预算
                  时保持诚实占位,不虚构上限。 */}
              <div className="px-md py-sm rounded-xl bg-surface-container border border-outline-variant/10 space-y-xs">
                {peakLabel != null && (
                  <div className="flex items-center justify-between gap-sm" data-testid="session-usage-peak">
                    <span
                      className="font-body-sm text-on-surface-variant truncate"
                      title={t('sidebar.sessions.contextPeak.title', {
                        tokens: sessionPeak != null ? fmtCompactTokens(sessionPeak, intl.locale) : peakLabel,
                      })}
                    >
                      {t('chat.input.usage.sessionPeak')}
                    </span>
                    <span className="font-body-sm text-on-surface tabular-nums">{peakLabel}</span>
                  </div>
                )}
                <div className="flex items-center justify-between gap-sm">
                  <span className="font-body-sm text-on-surface-variant truncate tabular-nums">
                    {hasCap
                      ? `${usd(spent)} / ${usd(budget! - spent)} ${t('chat.input.usage.budgetRemaining')}`
                      : t('budget.dialog.label')}
                  </span>
                  <Button
                    variant="outline"
                    size="sm"
                    className="cursor-pointer shrink-0"
                    onClick={() => setBudgetOpen(true)}
                  >
                    <span className="material-symbols-outlined icon-sm mr-xs" aria-hidden="true">payments</span>
                    {t('budget.menu.set')}
                  </Button>
                </div>
              </div>
            </>
          ) : (
            <p className="font-body-sm text-on-surface-variant text-center py-lg">
              {t('chat.input.usage.noSession')}
            </p>
          )}
        </ModalBody>
        <ModalFooter className="pt-0">
          <Button
            variant="ghost"
            disabled={!currentSessionId}
            title={t('chat.input.usage.viewAll')}
            className="mr-auto px-md py-sm rounded-xl text-primary hover:bg-primary/10 cursor-pointer disabled:opacity-40"
            onClick={() => { onClose(); navigate('/usage') }}
          >
            <span className="material-symbols-outlined icon-md mr-xs" aria-hidden="true">monitoring</span>
            {t('chat.input.usage.viewAll')}
          </Button>
          <Button variant="ghost" onClick={onClose} className="px-md py-sm rounded-xl text-on-surface-variant hover:bg-surface-container cursor-pointer">
            {t('budget.dialog.cancel')}
          </Button>
        </ModalFooter>
      </Modal>
      <BudgetDialog
        open={budgetOpen}
        sessionId={currentSessionId}
        budget={budget}
        onClose={() => setBudgetOpen(false)}
        onSaved={() => refreshBudget()}
      />
    </>
  )
}
