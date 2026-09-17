// PlanPanel — the right dock's 计划 tab (P0-③, ZCode delta ②).
//
// Surfaces the engine's persisted plan document as a first-class panel next
// to the conversation: `<workingDir>/.shannon/plans/*.md` via the
// `get_session_plan` command, refreshed on session/working-dir change, on
// any plan-tool result (enter/exit/get_plan_status), and on query
// completion. The lifecycle stays engine-owned — this panel is read-only.

import { useCallback, useEffect, useState } from 'react'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Markdown } from '@/components/chat/Markdown'
import { useT } from '@/i18n'
import { EVENT_NAMES, type SessionPlan } from '@/types'
import * as api from '@/lib/tauri-api'

interface PlanPanelProps {
  /** Session working directory (absolute path); null when unbound. */
  workingDir: string | null
  /** True while the composer's plan mode chip is active. */
  planModeActive: boolean
}

/** Fetch + live-refresh the working dir's latest plan. */
export function useSessionPlan(workingDir: string | null) {
  const [plan, setPlan] = useState<SessionPlan | null>(null)

  const refresh = useCallback(async () => {
    if (!workingDir) {
      setPlan(null)
      return
    }
    try {
      setPlan(await api.getSessionPlan(workingDir))
    } catch {
      // The panel is opportunistic (plans may not exist / dir unreadable) —
      // render the empty state instead of surfacing an error toast.
      setPlan(null)
    }
  }, [workingDir])

  useEffect(() => { void refresh() }, [refresh])

  useEffect(() => {
    const unlisteners: UnlistenFn[] = []
    let cancelled = false
    async function register() {
      const handlers = [
        // Any plan-tool round-trip may have changed the plan doc.
        listen(EVENT_NAMES.QUERY_TOOL_RESULT, (e) => {
          const p = e.payload as { tool_name?: string; result?: string }
          if (p.tool_name && p.tool_name.toLowerCase().includes('plan')) void refresh()
        }),
        listen(EVENT_NAMES.QUERY_COMPLETED, () => { void refresh() }),
      ]
      const results = await Promise.all(handlers)
      if (cancelled) {
        results.forEach(fn => fn())
        return
      }
      unlisteners.push(...results)
    }
    register()
    return () => {
      cancelled = true
      unlisteners.forEach(fn => fn())
    }
  }, [refresh])

  return { plan, refresh }
}

export default function PlanPanel({ workingDir, planModeActive }: PlanPanelProps) {
  const t = useT()
  const { plan, refresh } = useSessionPlan(workingDir)

  const approved = plan?.status === 'approved'

  return (
    <div className="space-y-md" data-testid="plan-panel">
      <div className="flex items-center gap-sm">
        <span className="material-symbols-outlined icon-sm text-primary shrink-0" aria-hidden="true">route</span>
        <h3 className="font-label-md text-on-surface uppercase tracking-wider opacity-60 flex-1 truncate">
          {t('chat.plan.title')}
        </h3>
        {plan && (
          <Badge size="sm" variant={approved ? 'primary' : 'neutral'} className={approved ? 'bg-primary/10 text-primary' : 'bg-surface-container-high text-on-surface-variant'}>
            <span className="material-symbols-outlined text-[12px] mr-[2px]" aria-hidden="true">{approved ? 'check_circle' : 'pending'}</span>
            {t(approved ? 'chat.plan.status.approved' : 'chat.plan.status.pending')}
          </Badge>
        )}
        <Button
          variant="ghost"
          size="icon-xs"
          onClick={() => void refresh()}
          aria-label={t('chat.plan.refresh.aria')}
          title={t('chat.plan.refresh.aria')}
          className="text-on-surface-variant hover:text-primary"
        >
          <span className="material-symbols-outlined text-[14px]" aria-hidden="true">refresh</span>
        </Button>
      </div>

      {planModeActive && (
        <p
          role="status"
          className="px-md py-xs rounded-lg bg-tertiary-container/40 text-on-tertiary-container font-label-sm flex items-center gap-xs"
        >
          <span className="material-symbols-outlined text-[14px]" aria-hidden="true">route</span>
          {t('chat.plan.modeActive')}
        </p>
      )}

      {plan && plan.content.trim() ? (
        <article className="p-md bg-surface-container rounded-xl border border-outline-variant/10">
          <div className="font-label-xs text-on-surface-variant mb-sm font-mono truncate" title={plan.created_at}>
            {plan.title}
          </div>
          <div className="font-body-sm text-on-surface prose prose-sm max-w-none prose-p:my-1 prose-pre:bg-surface-container-lowest prose-pre:p-sm prose-pre:rounded-lg prose-code:text-primary prose-code:before:content-[''] prose-code:after:content-['']">
            <Markdown>{plan.content}</Markdown>
          </div>
        </article>
      ) : (
        <div className="p-md rounded-xl border border-dashed border-outline-variant/30 text-center">
          <span className="material-symbols-outlined icon-md text-on-surface-variant/60" aria-hidden="true">note_add</span>
          <p className="font-label-md text-on-surface-variant mt-xs">{t('chat.plan.empty')}</p>
          <p className="font-label-sm text-on-surface-variant/70 mt-xs">{t('chat.plan.emptyHint')}</p>
        </div>
      )}
    </div>
  )
}
