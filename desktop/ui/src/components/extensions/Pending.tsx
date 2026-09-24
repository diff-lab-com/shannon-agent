// Extensions → Pending (IA X1, 评审裁决 #2).
//
// The single "needs my review" surface inside the Extensions domain:
//   1. 技能提案审查 (primary) — the skill-candidate review queue (the same
//      approve/reject path the inbox entry resolves through, T5) plus the
//      task-loop proposal drafts (the former global toast/panel UI, now
//      embedded here);
//   2. 错误区 — failed MCP connections / install errors (Claude Code Errors
//      tab semantics). No subscribable error source exists yet, so the
//      section ships as an empty-state placeholder (data source tracked for
//      a later task — no backend invented here).
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

  return (
    <div className="max-w-[900px] mx-auto px-lg py-lg pb-[64px] space-y-xl">
      {/* Section 1 — skill proposal review (primary area) */}
      <section aria-labelledby="extensions-pending-skills-title">
        <div className="mb-md">
          <h2 id="extensions-pending-skills-title" className="font-headline-md text-[20px] font-bold text-on-surface leading-tight">
            {t('extensions.pending.skills.title')}
          </h2>
          <p className="font-body-sm text-on-surface-variant mt-xs">{t('extensions.pending.skills.subtitle')}</p>
        </div>
        <SkillCandidateReviewQueue focusCandidateId={focusCandidateId} />
        {/* Task-loop proposal drafts — the former global review panel,
            embedded (IA X1). Renders nothing while there are none. */}
        <SkillProposalReviewPanel variant="inline" open onClose={() => {}} />
      </section>

      {/* Section 2 — errors (Claude Code Errors tab semantics). Placeholder
          until a subscribable MCP/install error source exists. */}
      <section aria-labelledby="extensions-pending-errors-title">
        <div className="mb-md">
          <h2 id="extensions-pending-errors-title" className="font-headline-md text-[20px] font-bold text-on-surface leading-tight">
            {t('extensions.pending.errors.title')}
          </h2>
          <p className="font-body-sm text-on-surface-variant mt-xs">{t('extensions.pending.errors.subtitle')}</p>
        </div>
        <EmptyState
          icon="error"
          title={t('extensions.pending.errors.empty.title')}
          description={t('extensions.pending.errors.empty.description')}
        />
      </section>
    </div>
  )
}
