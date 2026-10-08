// ChatStatusBar — Aurora redesign 2026-10 (docs/design/ui-redesign-2026-10/
// pages/02-chat.html 状态条): a quiet one-line strip under the composer.
//
// HONESTY CONTRACT (task ruling): this bar renders only data that actually
// exists — each segment independently hides when its source is absent, and
// nothing is ever mocked up to fill the line:
//   * working directory — the session's `working_dir` (ComposerPanel's
//     source), doubling as the app's only WD-picker entry point (the old
//     footer row moved into this bar verbatim: same button, aria label,
//     breadcrumb format and disabled-without-session behavior);
//   * session spend + budget remaining — `useSessionBudget` (the same hook
//     the RightDock Context tab and the header budget badge use); the spend
//     segment hides until the session ledger reports a cost, the budget
//     segment only exists while a > 0 cap is configured;
//   * context occupancy % — computed from the streaming UsagePayload's
//     `max_tokens` exactly like the Context tab's window bar (ContextPanel).
//     No `max_tokens` on the wire → no percentage (never a guessed window).
//
// Design shows a git-branch segment and a cache-hit segment; neither has a
// data source on this surface today, so neither renders (no decoration).

import { useIntl } from 'react-intl'
import { Button } from '@/components/ui/button'
import { cn } from '@/lib/utils'
import { useT } from '@/i18n'
import { useSessionBudget } from '@/hooks/useSessionBudget'
import { formatDirBreadcrumb } from '@/pages/chat/utils'
import type { UsagePayload } from '@/types'

export interface ChatStatusBarProps {
  /** Session working dir (may be empty → "not set" segment). */
  workingDir: string
  /** Streaming usage snapshot — feeds the context-occupancy segment. */
  usage: UsagePayload | null
  /** Current session id — scopes the budget hook; null disables its fetch. */
  sessionId: string | null
  /** Working-directory picker action (ComposerPanel owns the shortcut). */
  onChangeWorkingDir: () => void
}

export function ChatStatusBar({ workingDir, usage, sessionId, onChangeWorkingDir }: ChatStatusBarProps) {
  const t = useT()
  const intl = useIntl()
  const { budget, usage: sessionUsage } = useSessionBudget(sessionId)

  // Context occupancy — same math as the Context tab's window bar: sent+
  // received tokens over the reported window, clamped, integer percent.
  // Aurora 2026-10 (02 状态条): the segment also spells out the raw
  // "used/total" token counts (设计稿: 上下文 38% · 74k/200k) — both numbers
  // come from the same UsagePayload, so the detail is always real when the
  // percent renders.
  const contextPct = (() => {
    const total = (usage?.input_tokens ?? 0) + (usage?.output_tokens ?? 0)
    const max = usage?.max_tokens
    if (!max || total <= 0) return null
    return Math.min(100, Math.round((total / max) * 100))
  })()
  const fmtCompact = (n: number) => {
    try {
      return new Intl.NumberFormat(intl.locale, { notation: 'compact', maximumFractionDigits: 1 }).format(n)
    } catch {
      return n.toLocaleString(intl.locale)
    }
  }
  const contextUsed = contextPct != null
    ? fmtCompact((usage?.input_tokens ?? 0) + (usage?.output_tokens ?? 0))
    : ''
  const contextTotal = contextPct != null && usage?.max_tokens ? fmtCompact(usage.max_tokens) : ''

  // Session spend — only once the session ledger has observed something.
  const sessionCost = sessionUsage?.cost_usd
  const hasCost = sessionCost != null && sessionCost > 0
  // Budget remaining — only while a positive cap is configured.
  const budgetLeft = budget != null && budget > 0 ? budget - (sessionUsage?.cost_usd ?? 0) : null

  // The working-dir segment always renders (set or "not set" — both real);
  // the spacer only matters when a right-hand segment joins it.
  const hasRightSegments = contextPct != null || hasCost || budgetLeft != null

  const fmtUsd = (n: number) =>
    new Intl.NumberFormat(intl.locale, {
      style: 'currency',
      currency: 'USD',
      minimumFractionDigits: 2,
      maximumFractionDigits: n < 1 ? 4 : 2,
    }).format(n)

  return (
    <div
      data-testid="chat-status-bar"
      aria-label={t('chat.statusbar.aria')}
      className="mt-xs flex items-center gap-md px-sm text-label-sm text-on-surface-variant min-w-0"
    >
      {/* Working directory — the WD-picker entry (formerly the standalone
          footer row): same aria label, breadcrumb format, disabled state. */}
      <Button
        type="button"
        variant="ghost"
        onClick={onChangeWorkingDir}
        disabled={!sessionId}
        className="flex items-center gap-xs min-w-0 max-w-[50%] hover:text-primary transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
        title={workingDir || t('chat.input.footer.workingDir.unset')}
        aria-label={t('chat.input.footer.workingDir.aria')}
      >
        <span className="material-symbols-outlined icon-sm shrink-0">folder</span>
        <span className="truncate font-mono">
          {workingDir ? formatDirBreadcrumb(workingDir) : t('chat.input.footer.workingDir.unset')}
        </span>
      </Button>

      {hasRightSegments && <span className="flex-1 min-w-0" aria-hidden="true" />}

      {contextPct != null && (
        <span
          data-testid="chat-status-context"
          className="flex items-center gap-xs shrink-0 tabular-nums"
          title={t('chat.statusbar.context.tokens.title', { percent: contextPct, used: contextUsed, total: contextTotal })}
        >
          <span className="material-symbols-outlined icon-sm shrink-0" aria-hidden="true">data_usage</span>
          {t('chat.statusbar.context.tokens', { percent: contextPct, used: contextUsed, total: contextTotal })}
        </span>
      )}

      {hasCost && sessionCost != null && (
        <span
          data-testid="chat-status-cost"
          className="flex items-center gap-xs shrink-0 tabular-nums"
          title={t('chat.statusbar.cost.title')}
        >
          <span className="material-symbols-outlined icon-sm shrink-0" aria-hidden="true">payments</span>
          {t('chat.statusbar.cost', { cost: fmtUsd(sessionCost) })}
        </span>
      )}

      {budgetLeft != null && (
        <span
          data-testid="chat-status-budget-left"
          className={cn(
            'flex items-center gap-xs shrink-0 tabular-nums',
            budgetLeft <= 0 && 'text-error',
          )}
          title={t('chat.statusbar.budgetLeft.title')}
        >
          <span className="material-symbols-outlined icon-sm shrink-0" aria-hidden="true">savings</span>
          {t('chat.statusbar.budgetLeft', { amount: fmtUsd(Math.max(0, budgetLeft)) })}
        </span>
      )}
    </div>
  )
}

export default ChatStatusBar
