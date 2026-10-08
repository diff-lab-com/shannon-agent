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
//     `context_total` (the engine-resolved context window) exactly like the
//     Context tab's window bar (ContextPanel). No `context_total` on the
//     wire → no percentage (never a guessed window); the session rail's
//     峰值 chip carries the persisted window peak instead.
//
// Design shows a git-branch segment and a cache-hit segment — both wired
// now (2026-10-08 status-bar round): branch = `current_git_branch` IPC on
// workingDir change (per-dir memo), cache = UsagePayload.cache_hit_rate
// (the wire field existed but was first populated in the same round). Each
// still hides independently when its source is absent.

import { useEffect, useRef, useState } from 'react'
import { useIntl } from 'react-intl'
import { Button } from '@/components/ui/button'
import { cn } from '@/lib/utils'
import { useT } from '@/i18n'
import { useSessionBudget } from '@/hooks/useSessionBudget'
import { formatDirBreadcrumb } from '@/pages/chat/utils'
import * as api from '@/lib/tauri-api'
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
  // received tokens over the frame's resolved context window
  // (`context_total`), clamped, integer percent. 上下文峰值 (批 1): the
  // denominator is the ENGINE-RESOLVED window — absent → the segment hides
  // (never a guessed window; the rail row shows the persisted session peak
  // instead). Aurora 2026-10 (02 状态条): the segment also spells out the
  // raw "used/total" token counts (设计稿: 上下文 38% · 74k/200k) — both
  // numbers come from the same UsagePayload, so the detail is always real
  // when the percent renders.
  const contextPct = (() => {
    const total = (usage?.input_tokens ?? 0) + (usage?.output_tokens ?? 0)
    const max = usage?.context_total
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
  const contextTotal = contextPct != null && usage?.context_total ? fmtCompact(usage.context_total) : ''

  // Session spend — only once the session ledger has observed something.
  const sessionCost = sessionUsage?.cost_usd
  const hasCost = sessionCost != null && sessionCost > 0
  // Budget remaining — only while a positive cap is configured.
  const budgetLeft = budget != null && budget > 0 ? budget - (sessionUsage?.cost_usd ?? 0) : null

  // Branch segment — one IPC per distinct working dir (memoized for the
  // session-switch back-and-forth); the segment hides for non-git dirs and
  // while the lookup is in flight (a stale branch never renders).
  const [branch, setBranch] = useState<string | null>(null)
  const branchCacheRef = useRef<Map<string, string | null>>(new Map())
  useEffect(() => {
    if (!workingDir) {
      setBranch(null)
      return
    }
    const cached = branchCacheRef.current.get(workingDir)
    if (cached !== undefined) {
      setBranch(cached)
      return
    }
    let cancelled = false
    void api.currentGitBranch(workingDir).then((b) => {
      branchCacheRef.current.set(workingDir, b)
      if (!cancelled) setBranch(b)
    }).catch(() => {
      branchCacheRef.current.set(workingDir, null)
      if (!cancelled) setBranch(null)
    })
    return () => { cancelled = true }
  }, [workingDir])

  // Cache-hit segment — per-frame fraction straight off the UsagePayload
  // (populated backend-side since 2026-10-08; absent → hidden).
  const cachePct = usage?.cache_hit_rate != null
    ? Math.round(usage.cache_hit_rate * 100)
    : null

  // The working-dir segment always renders (set or "not set" — both real);
  // the spacer only matters when a right-hand segment joins it.
  const hasRightSegments = branch != null || contextPct != null || cachePct != null || hasCost || budgetLeft != null

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

      {branch != null && (
        <span
          data-testid="chat-status-branch"
          className="flex items-center gap-xs shrink-0 font-mono min-w-0"
          title={t('chat.statusbar.branch.title')}
        >
          <span className="material-symbols-outlined icon-sm shrink-0" aria-hidden="true">account_tree</span>
          <span className="truncate">{branch}</span>
        </span>
      )}

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

      {cachePct != null && (
        <span
          data-testid="chat-status-cache"
          className="flex items-center gap-xs shrink-0 tabular-nums"
          title={t('chat.statusbar.cache.title')}
        >
          <span className="material-symbols-outlined icon-sm shrink-0" aria-hidden="true">bolt</span>
          {t('chat.statusbar.cache', { percent: cachePct })}
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
