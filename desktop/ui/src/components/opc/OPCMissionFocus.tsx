import { useEffect, useMemo, useState } from 'react'
import { useIntl } from 'react-intl'
import { toast } from 'sonner'
import { Button } from '@/components/ui/button'
import { toastError } from '@/lib/errorToast'
import * as api from '@/lib/tauri-api'
import { classifyStatus } from '@/lib/task-status'
import { fmtUsd } from '@/components/opc/MissionCard'
import type { MissionConfig, MissionProgress, TaskItem } from '@/types'

interface Props {
  config: { provider?: string; strategic_focus?: string } | null
  /** Board tasks — the hero aggregates 进行中/已完成 counts + progress from
   *  them (design 09:169-181). */
  tasks: TaskItem[]
  /** The configured mission (config.mission). G4 (R2 audit §4-G4): when
   *  present the hero renders the MISSION mode — one aurora card whose
   *  headline is the mission name and whose strip carries the design-09
   *  三要素 (进度/预算/截止); the MissionCard below collapses into the
   *  「管理使命」dialog owned by the page. `null` keeps the legacy
   *  free-text strategic-focus hero. */
  mission?: MissionConfig | null
  /** Opens the manage-mission dialog (OPC.tsx owns the Dialog + the
   *  MissionCard editor). Absent → the manage affordance hides. */
  onManageMission?: () => void
}

export default function OPCMissionFocus({ config, tasks, mission = null, onManageMission }: Props) {
  const intl = useIntl()
  const [editing, setEditing] = useState(false)
  const [text, setText] = useState('')

  const focus = config?.strategic_focus
    || (config?.provider
      ? `${config.provider.charAt(0).toUpperCase() + config.provider.slice(1)} Agent Orchestration — autonomous task execution with multi-agent coordination.`
      : intl.formatMessage({ id: 'opc.missionFocus.defaultFocus' }))

  useEffect(() => { setText(focus) }, [focus])

  // 使命 hero 三要素的纯前端部分 (audit §09 P1): counts + progress bar +
  // aurora line — all derived from the tasks the page already holds.
  // Catalog may not have hydrated (tests, degraded states) — never assume
  // the array exists (OPCMissionFocus tests mock useCatalog without tasks).
  const safeTasks = useMemo(() => tasks ?? [], [tasks])
  const { activeCount, doneCount, progressPct } = useMemo(() => {
    let active = 0
    let done = 0
    for (const task of safeTasks) {
      const family = classifyStatus(task.status)
      if (family === 'active') active++
      else if (family === 'done') done++
    }
    return {
      activeCount: active,
      doneCount: done,
      progressPct: safeTasks.length > 0 ? Math.round((done / safeTasks.length) * 100) : 0,
    }
  }, [safeTasks])

  // G4: the hero strip's 花费 figure comes from the same `mission_progress`
  // projection MissionCard reads — queried here too so the hero stays
  // self-sufficient (the dialog is open rarely; the projection is cheap).
  // Follows the mission's *content* like MissionCard does, so a save in the
  // manage dialog (refreshConfig → new mission object) re-queries and the
  // chip never goes stale after an edit.
  const missionKey = useMemo(() => (mission ? JSON.stringify(mission) : null), [mission])
  const [progress, setProgress] = useState<MissionProgress | null>(null)
  useEffect(() => {
    if (!missionKey) {
      setProgress(null)
      return
    }
    let cancelled = false
    api.missionProgress()
      .then((p) => { if (!cancelled) setProgress(p) })
      .catch(() => { if (!cancelled) setProgress(null) })
    return () => { cancelled = true }
  }, [missionKey])

  const save = () => {
    api.configure({ key: 'strategic_focus', value: text })
      .then(() => toast.success(intl.formatMessage({ id: 'opc.missionFocus.focusSaved' })))
      .catch((e) => toastError(intl.formatMessage({ id: 'opc.missionFocus.saveFailed' }), e))
    setEditing(false)
  }

  // ── Mission mode (G4): single hero, design 09:169-181 ────────────────────
  if (mission) {
    // Honesty contract (mirrors MissionCard): a chip renders only when its
    // field exists — no budget → no budget chip, no measured spend → no
    // 花费 half, a past deadline gets the muted 已逾期 marker.
    const budget = mission.budget_usd
    const spent = progress?.budget_used_usd
    const overdue = mission.deadline_ts != null && mission.deadline_ts < Date.now()
    return (
      <div className="bg-surface-container-lowest rounded-2xl p-xl mb-lg border border-outline-variant/30 relative shadow-e1 aurora-line">
        <div className="flex items-start gap-md">
          <span className="material-symbols-outlined text-primary mt-xs" style={{ fontSize: 30 }} aria-hidden="true">
            target
          </span>
          <div className="flex-1 min-w-0">
            <div className="flex items-center justify-between mb-sm">
              <div className="flex items-center gap-sm uppercase font-label-md text-label-sm tracking-widest text-on-surface-variant font-bold">
                <span className="w-1.5 h-1.5 bg-outline-variant rotate-45 block" />
                {intl.formatMessage({ id: 'opc.missionFocus.todayMission' })}
              </div>
              {onManageMission && (
                <Button
                  variant="link"
                  size="sm"
                  className="text-label-sm h-auto px-0 hover:underline"
                  onClick={onManageMission}
                  data-testid="mission-manage-open"
                >
                  {intl.formatMessage({ id: 'opc.missionFocus.manage' })}
                </Button>
              )}
            </div>
            <h2 className="font-headline-lg text-[28px] font-bold text-on-surface mt-sm max-w-5xl break-words">
              {mission.name}
            </h2>
            {/* Progress + counts strip (design 09:174-177) — the 进度 of the
                三要素, same aggregate as the legacy hero. */}
            <div
              className="mt-sm"
              role="status"
              aria-label={intl.formatMessage({ id: 'opc.missionFocus.progress.aria' }, { percent: progressPct })}
              data-testid="opc-mission-progress"
            >
              <div className="h-1.5 w-full max-w-xl bg-surface-container rounded-full overflow-hidden">
                <div
                  className="h-full bg-primary rounded-full transition-all duration-(--duration-slower)"
                  style={{ width: `${progressPct}%` }}
                />
              </div>
              <div className="flex items-center gap-sm mt-xs font-label-sm text-label-sm text-on-surface-variant flex-wrap">
                <span>
                  {intl.formatMessage({ id: 'opc.missionFocus.activeCount' }, { count: activeCount })}
                </span>
                <span aria-hidden="true">·</span>
                <span>
                  {intl.formatMessage({ id: 'opc.missionFocus.doneCount' }, { count: doneCount })}
                </span>
                <span aria-hidden="true">·</span>
                <span className="tabular-nums">
                  {intl.formatMessage({ id: 'opc.missionFocus.totalCount' }, { count: safeTasks.length })}
                </span>
                {/* 预算 (spend shown only when the ledger measured some) */}
                {budget != null && (
                  <>
                    <span aria-hidden="true">·</span>
                    <span className="tabular-nums" data-testid="mission-hero-budget">
                      {spent != null
                        ? intl.formatMessage({ id: 'opc.missionFocus.spentOfBudget' }, { spent: fmtUsd(spent), budget: fmtUsd(budget) })
                        : intl.formatMessage({ id: 'opc.missionFocus.budgetOnly' }, { budget: fmtUsd(budget) })}
                    </span>
                  </>
                )}
                {/* 截止 */}
                {mission.deadline_ts != null && (
                  <>
                    <span aria-hidden="true">·</span>
                    <span data-testid="mission-hero-deadline">
                      {intl.formatMessage({ id: 'opc.missionFocus.deadline' }, {
                        date: intl.formatDate(mission.deadline_ts, { month: 'short', day: 'numeric' }),
                      })}
                      {overdue && (
                        <span className="text-on-surface-variant/70" data-testid="mission-hero-overdue">
                          {' '}· {intl.formatMessage({ id: 'opc.missionCard.overdue' })}
                        </span>
                      )}
                    </span>
                  </>
                )}
              </div>
            </div>
          </div>
        </div>
      </div>
    )
  }

  // ── Legacy free-text hero (no mission configured — unchanged) ────────────
  return (
    <div className="bg-surface-container-lowest rounded-2xl p-xl mb-lg border border-outline-variant/30 relative shadow-e1 aurora-line">
      <div className="flex items-start gap-md">
        <span className="material-symbols-outlined text-primary mt-xs" style={{ fontSize: 30 }} aria-hidden="true">
          target
        </span>
        <div className="flex-1 min-w-0">
          <div className="flex items-center justify-between mb-sm">
            <div className="flex items-center gap-sm uppercase font-label-md text-label-sm tracking-widest text-on-surface-variant font-bold">
              <span className="w-1.5 h-1.5 bg-outline-variant rotate-45 block" />
              {intl.formatMessage({ id: 'opc.missionFocus.todayMission' })}
            </div>
            <Button
              variant="link"
              size="sm"
              className="text-label-sm h-auto px-0 hover:underline"
              onClick={() => setEditing(!editing)}
              aria-expanded={editing}
            >
              {editing ? intl.formatMessage({ id: 'opc.missionFocus.cancel' }) : intl.formatMessage({ id: 'opc.missionFocus.edit' })}
            </Button>
          </div>
          {editing ? (
            <div className="mt-sm space-y-md">
              <textarea
                className="w-full h-24 p-md bg-surface-container-low rounded-xl border border-outline-variant/30 text-body-md resize-none focus:outline-none focus:ring-2 focus:ring-primary/30"
                value={text}
                onChange={e => setText(e.target.value)}
                aria-label={intl.formatMessage({ id: 'opc.missionFocus.editMission.aria' })}
              />
              <Button
                className="px-md py-sm rounded-lg font-label-md hover:opacity-90"
                onClick={save}
              >
                {intl.formatMessage({ id: 'opc.missionFocus.saveFocus' })}
              </Button>
            </div>
          ) : (
            <h2 className="font-headline-lg text-[28px] font-bold text-on-surface mt-sm max-w-5xl">
              {focus}
            </h2>
          )}
          {/* Progress + counts strip (design 09:174-177) — visible in both
              states so the mission reads at a glance while editing too. */}
          <div
            className="mt-sm"
            role="status"
            aria-label={intl.formatMessage({ id: 'opc.missionFocus.progress.aria' }, { percent: progressPct })}
            data-testid="opc-mission-progress"
          >
            <div className="h-1.5 w-full max-w-xl bg-surface-container rounded-full overflow-hidden">
              <div
                className="h-full bg-primary rounded-full transition-all duration-(--duration-slower)"
                style={{ width: `${progressPct}%` }}
              />
            </div>
            <div className="flex items-center gap-sm mt-xs font-label-sm text-label-sm text-on-surface-variant flex-wrap">
              <span>
                {intl.formatMessage({ id: 'opc.missionFocus.activeCount' }, { count: activeCount })}
              </span>
              <span aria-hidden="true">·</span>
              <span>
                {intl.formatMessage({ id: 'opc.missionFocus.doneCount' }, { count: doneCount })}
              </span>
              <span aria-hidden="true">·</span>
              <span className="tabular-nums">
                {intl.formatMessage({ id: 'opc.missionFocus.totalCount' }, { count: safeTasks.length })}
              </span>
            </div>
          </div>
        </div>
      </div>
    </div>
  )
}
