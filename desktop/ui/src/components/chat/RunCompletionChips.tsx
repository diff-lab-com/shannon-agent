// RunCompletionChips — D5 方案① (主动任务推荐): static post-completion
// action chips under the finished run, rendered by MessageArea once the run
// has settled.
//
// Rules (all derived from data the frontend already has — the run terminal
// status + the finished run's tool calls; NO backend fields were added, and
// ZERO model calls are spent):
//   - run failed            → 「重试」+「分析失败原因」 (2 chips);
//   - run succeeded and the run contained ≥1 file-mutating tool call
//     (the same `classifyTool` Changes bucket lib/toolGrouping.ts uses,
//     retained per-run as `runProcess.hadChanges`) → 「提交这些改动」 (1 chip);
//   - otherwise             → nothing (noise avoidance is a spec requirement).
//
// Interaction contract: a chip only FILLS the composer (the exact
// `setInput` mechanism WelcomeState's example cards use) — it never
// auto-sends. The chips are keyed off `runProcess`, so the next send
// (beginRun resets the snapshot to running) and session switches (reset to
// idle) dismiss them for free. Everything is gated on the
// `suggestions.enabled` presentation toggle (default ON; config read
// failures stay silent and keep the feature shown).

import { useMemo } from 'react'
import { useIntl } from 'react-intl'
import { Button } from '@/components/ui/button'
import { useChat } from '@/context/ChatContext'
import { useCatalog } from '@/context/CatalogContext'
import { useComposer } from '@/pages/chat/ComposerContext'
import { initialRunProcess } from '@/lib/runProcess'

export default function RunCompletionChips() {
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })
  // Absent snapshot (partial harness / session not yet bound) = idle = no
  // chips — a suggestion must never render off missing data.
  const { messages, runProcess = initialRunProcess(), isQuerying } = useChat()
  const { config } = useCatalog()
  const { setInput } = useComposer()

  // Retry affordance: re-fill the composer with the last user message so
  // one Enter re-runs the failed turn. Fills, never sends. (Hook before any
  // early return — the gates below decide only whether it is rendered.)
  const lastUser = useMemo(() => {
    for (let i = messages.length - 1; i >= 0; i--) {
      if (messages[i]?.role === 'user') return messages[i]
    }
    return null
  }, [messages])

  // Presentation gate — same read pattern as GeneralSettings: config
  // missing (not loaded / old backend) defaults to enabled, and a read
  // failure never surfaces (the feature simply shows).
  if ((config?.suggestions_enabled ?? true) === false) return null
  // Only a SETTLED run suggests anything; a live run (or the send that
  // started a new one) hides the chips until it settles again.
  if (isQuerying) return null

  if (runProcess.status === 'failed') {
    return (
      <div
        data-testid="run-completion-chips"
        className="flex flex-wrap items-center gap-sm pt-md"
        aria-label={t('chat.completionChips.aria')}
      >
        {lastUser && (
          <Chip
            testId="run-completion-chip-retry"
            icon="refresh"
            label={t('chat.completionChips.retry')}
            onClick={() => setInput(lastUser.content)}
          />
        )}
        <Chip
          testId="run-completion-chip-analyze-failure"
          icon="troubleshoot"
          label={t('chat.completionChips.analyzeFailure')}
          onClick={() => setInput(t('chat.completionChips.analyzeFailure.prompt'))}
        />
      </div>
    )
  }

  if (runProcess.status === 'done' && runProcess.hadChanges) {
    return (
      <div
        data-testid="run-completion-chips"
        className="flex flex-wrap items-center gap-sm pt-md"
        aria-label={t('chat.completionChips.aria')}
      >
        <Chip
          testId="run-completion-chip-commit-changes"
          icon="commit"
          label={t('chat.completionChips.commitChanges')}
          onClick={() => setInput(t('chat.completionChips.commitChanges.prompt'))}
        />
      </div>
    )
  }

  // Succeeded without file changes / cancelled-clean / idle: no chips.
  return null
}

function Chip({ icon, label, testId, onClick }: { icon: string; label: string; testId: string; onClick: () => void }) {
  return (
    <Button
      type="button"
      variant="outline"
      data-testid={testId}
      onClick={onClick}
      className="px-md py-xs rounded-full font-label-sm text-on-surface-variant bg-surface-container-lowest border border-outline-variant/30 hover:bg-surface-container-high hover:border-primary/30 hover:text-primary cursor-pointer"
    >
      <span className="material-symbols-outlined icon-md" aria-hidden="true">{icon}</span>
      {label}
    </Button>
  )
}
