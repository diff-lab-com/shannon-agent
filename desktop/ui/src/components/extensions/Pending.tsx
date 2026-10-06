// Extensions → Pending (IA X1, 评审裁决 #2).
//
// The single "needs my review" surface inside the Extensions domain:
//   1. 技能提案审查 (primary) — the skill-candidate review queue (the same
//      approve/reject path the inbox entry resolves through, T5) plus the
//      task-loop proposal drafts (the former global toast/panel UI, now
//      embedded here);
//   2. 错误区 — failed MCP connections / install errors (Claude Code Errors
//      tab semantics). W1-7 (R2-P1-6): the backend now reports each MCP
//      server's pool-level failure via `list_mcp_servers` (`last_error`),
//      so this section renders the real error list; the empty state is only
//      the honest "nothing failed" fallback.
//
// Triage's skill-candidate cards hand over `skillCandidateId` via router
// state; like the inbox highlight (IA T2) it is a one-shot focus and the
// state is drained immediately.

import { useEffect, useState } from 'react'
import { useLocation, useNavigate } from 'react-router-dom'
import { useIntl } from 'react-intl'
import EmptyState from '@/components/ui/empty-state'
import SkillCandidateReviewQueue from '@/components/skills/SkillCandidateReviewQueue'
import SkillProposalReviewPanel from '@/components/skills/SkillProposalReviewPanel'
import { listMcpServers } from '@/lib/tauri-api'
import type { McpServerInfo } from '@/types'

export default function Pending() {
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })
  const navigate = useNavigate()
  const location = useLocation()

  const [focusCandidateId] = useState<string | null>(
    () => (location.state as { skillCandidateId?: string } | null)?.skillCandidateId ?? null,
  )
  useEffect(() => {
    if (focusCandidateId == null) return
    navigate(location.pathname, { replace: true })
    // Run once per mount — the point is to drain the router state.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  // W1-7 (R2-P1-6): real MCP connection failures. The backend `last_error`
  // is the process pool's own failure reason (spawn error, failed health
  // check) — no invented data, just what the pool reported.
  const [mcpFailures, setMcpFailures] = useState<McpServerInfo[]>([])
  const [mcpErrorsLoadFailed, setMcpErrorsLoadFailed] = useState(false)
  useEffect(() => {
    let cancelled = false
    listMcpServers()
      .then((rows) => {
        if (cancelled) return
        setMcpFailures(rows.filter((srv) => !!srv.last_error && !srv.connected))
        setMcpErrorsLoadFailed(false)
      })
      .catch(() => {
        if (!cancelled) setMcpErrorsLoadFailed(true)
      })
    return () => {
      cancelled = true
    }
  }, [])

  return (
    <div className="max-w-[900px] mx-auto px-lg py-lg pb-[64px] space-y-xl">
      {/* Section 1 — skill proposal review (primary area) */}
      <section aria-labelledby="extensions-pending-skills-title">
        <div className="mb-md">
          <h2 id="extensions-pending-skills-title" className="font-headline-md text-headline-sm font-bold text-on-surface leading-tight">
            {t('extensions.pending.skills.title')}
          </h2>
          <p className="font-body-sm text-on-surface-variant mt-xs">{t('extensions.pending.skills.subtitle')}</p>
        </div>
        <SkillCandidateReviewQueue focusCandidateId={focusCandidateId} />
        {/* Task-loop proposal drafts — the former global review panel,
            embedded (IA X1). Renders nothing while there are none. */}
        <SkillProposalReviewPanel variant="inline" open onClose={() => {}} />
      </section>

      {/* Section 2 — errors (Claude Code Errors tab semantics). W1-7: real
          MCP connection failures when there are any; the empty state is the
          true "nothing failed" fallback. */}
      <section aria-labelledby="extensions-pending-errors-title">
        <div className="mb-md">
          <h2 id="extensions-pending-errors-title" className="font-headline-md text-headline-sm font-bold text-on-surface leading-tight">
            {t('extensions.pending.errors.title')}
          </h2>
          <p className="font-body-sm text-on-surface-variant mt-xs">{t('extensions.pending.errors.subtitle')}</p>
        </div>
        {mcpErrorsLoadFailed ? (
          <p className="font-body-sm text-on-surface-variant">{t('extensions.pending.errors.loadFailed')}</p>
        ) : mcpFailures.length === 0 ? (
          <EmptyState
            icon="error"
            title={t('extensions.pending.errors.empty.title')}
            description={t('extensions.pending.errors.empty.description')}
          />
        ) : (
          <ul className="space-y-sm" data-testid="mcp-error-list">
            {mcpFailures.map((srv) => (
              <li
                key={srv.name}
                className="border border-error/30 rounded-xl bg-error/5 px-md py-sm"
              >
                <div className="flex items-center gap-xs">
                  <span className="material-symbols-outlined icon-sm text-error" aria-hidden="true">dns</span>
                  <span className="font-bold text-label-md text-on-surface">{srv.name}</span>
                </div>
                <p className="font-body-sm text-error mt-xs break-words font-mono">{srv.last_error}</p>
              </li>
            ))}
          </ul>
        )}
      </section>
    </div>
  )
}
