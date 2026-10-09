// MissionCard — 缓期批 2 使命卡 (single-mission v1).
//
// The config-backed「使命」entity the OPCMissionFocus hero was waiting for
// (audit B3 单独立项): name + optional budget/deadline + linked-task
// progress. Reads identity from `config.mission` (the configure('mission')
// chain — same family as strategic_focus) and progress from the
// `mission_progress` projection, so the card can never disagree with the
// board it mirrors.
//
// Honesty contract (mirrors the backend): absent data renders nothing —
// no budget_usd → no budget chip, no budget_used_usd → no usage line
// (never a $0 claim where nothing was measured), found:false → 「任务不
// 存在」 instead of invented state, a past deadline gets a muted 已逾期
// marker computed from the real clock.
//
// Hidden entirely when no mission is configured (mission_progress returns
// null / config.mission absent — absent stays absent).

import { useEffect, useMemo, useState } from 'react'
import { useIntl, type PrimitiveType } from 'react-intl'
import { toast } from 'sonner'
import { Button } from '@/components/ui/button'
import { toastError } from '@/lib/errorToast'
import * as api from '@/lib/tauri-api'
import { classifyStatus, STATUS_FAMILY } from '@/lib/task-status'
import type { MissionConfig, MissionProgress, MissionTaskProgress, TaskItem } from '@/types'

/** Backend configure bounds (commands_config.rs mission arm) mirrored for
 *  inline validation — the backend stays the authority, these only give
 *  the user early feedback in the editor. */
const MAX_NAME_LEN = 200
const MAX_TASK_IDS = 200
/** 2100-01-01Z epoch-ms — the backend's plausible-deadline upper bound. */
const MAX_DEADLINE_TS = 4102444800000

/** Two-decimal USD, byte-identical to TaskCard's cost chip formatting. */
function fmtUsd(n: number): string {
  return new Intl.NumberFormat('en-US', {
    style: 'currency',
    currency: 'USD',
    minimumFractionDigits: 2,
    maximumFractionDigits: 2,
  }).format(n)
}

interface EditForm {
  name: string
  /** Raw input text; '' = no budget. */
  budget: string
  /** `<input type="date">` value (yyyy-mm-dd); '' = no deadline. */
  deadline: string
  /** The full ordered id set to persist (board tasks + preserved links). */
  taskIds: string[]
}

function formFromMission(mission: MissionConfig | null): EditForm {
  return {
    name: mission?.name ?? '',
    budget: mission?.budget_usd != null ? String(mission.budget_usd) : '',
    deadline: mission?.deadline_ts != null
      ? new Date(mission.deadline_ts).toISOString().slice(0, 10)
      : '',
    taskIds: [...(mission?.task_ids ?? [])],
  }
}

/** Mirror of the backend's mission-arm validation. Returns one inline
 *  error per field; an all-empty object means the payload is submittable. */
function validate(form: EditForm): { name?: string; budget?: string; deadline?: string; taskIds?: string } {
  const errors: ReturnType<typeof validate> = {}
  const name = form.name.trim()
  if (!name) errors.name = 'opc.missionCard.edit.nameRequired'
  else if (name.length > MAX_NAME_LEN) errors.name = 'opc.missionCard.edit.nameTooLong'

  if (form.budget.trim() !== '') {
    const n = Number(form.budget)
    if (!Number.isFinite(n) || n < 0) errors.budget = 'opc.missionCard.edit.budgetInvalid'
  }

  if (form.deadline.trim() !== '') {
    const ts = new Date(form.deadline).getTime()
    if (!Number.isFinite(ts) || ts <= 0 || ts > MAX_DEADLINE_TS) {
      errors.deadline = 'opc.missionCard.edit.deadlineInvalid'
    }
  }

  // Checkbox-sourced ids are trimmed non-empty by construction; the only
  // backend rejection left reachable is the >200-distinct bound.
  if (new Set(form.taskIds).size > MAX_TASK_IDS) errors.taskIds = 'opc.missionCard.edit.tasksTooMany'
  return errors
}

interface Props {
  /** The configured mission, or null when none — the whole card hides. */
  mission: MissionConfig | null
  /** Board tasks — the editor's link picker. May be empty (board not
   *  hydrated); linked ids not on the board are preserved as chips. */
  tasks: TaskItem[]
  /** Re-read the config after a save (the configure chain also emits
   *  config-updated, which refreshes it app-wide — this keeps the
   *  save-to-display path deterministic in the same tick). */
  refreshConfig: () => Promise<void>
}

export default function MissionCard({ mission, tasks, refreshConfig }: Props) {
  const intl = useIntl()
  const t = (id: string, values?: Record<string, PrimitiveType>) =>
    intl.formatMessage({ id }, values)

  const [editing, setEditing] = useState(false)
  const [form, setForm] = useState<EditForm>(() => formFromMission(mission))
  const [saving, setSaving] = useState(false)
  const [progress, setProgress] = useState<MissionProgress | null>(null)
  const [progressFailed, setProgressFailed] = useState(false)
  const [progressLoading, setProgressLoading] = useState(false)

  // Progress follows the mission's *content* (not object identity) so an
  // unrelated config refresh doesn't re-query, while a real mission edit
  // (or a mission arriving/being cleared) always does.
  const missionKey = useMemo(() => (mission ? JSON.stringify(mission) : null), [mission])
  useEffect(() => {
    if (!missionKey) {
      setProgress(null)
      setProgressFailed(false)
      setProgressLoading(false)
      return
    }
    let cancelled = false
    setProgressLoading(true)
    setProgressFailed(false)
    api.missionProgress()
      .then((p) => { if (!cancelled) { setProgress(p); setProgressLoading(false) } })
      .catch(() => { if (!cancelled) { setProgressFailed(true); setProgressLoading(false) } })
    return () => { cancelled = true }
  }, [missionKey])

  if (!mission) return null

  const startEdit = () => { setForm(formFromMission(mission)); setEditing(true) }
  const errors = validate(form)

  const save = () => {
    if (Object.keys(errors).length > 0 || saving) return
    setSaving(true)
    const name = form.name.trim()
    const budget = form.budget.trim() === '' ? null : Number(form.budget)
    const deadline = form.deadline.trim() === '' ? null : new Date(form.deadline).getTime()
    // Dedupe first-occurrence, exactly like the backend would anyway.
    const taskIds = [...new Set(form.taskIds)]
    const payload: MissionConfig = {
      name,
      ...(budget != null ? { budget_usd: budget } : {}),
      ...(deadline != null ? { deadline_ts: deadline } : {}),
      task_ids: taskIds,
    }
    api.configure({ key: 'mission', value: JSON.stringify(payload) })
      .then(async () => {
        await refreshConfig()
        toast.success(t('opc.missionCard.saved'))
        // Close only on success — a rejected configure keeps the form open
        // with the user's input intact (the inline errors stay visible).
        setEditing(false)
      })
      .catch((e) => toastError(t('opc.missionCard.saveFailed'), e))
      .finally(() => setSaving(false))
  }

  // Linked ids the board no longer knows — preserved by the editor as
  // removable chips so a dead link is always visible AND unlinkable.
  const boardIds = new Set(tasks.map(task => task.id))
  const offBoardIds = form.taskIds.filter(id => !boardIds.has(id))

  const overdue = mission.deadline_ts != null && mission.deadline_ts < Date.now()
  const usage = progress?.budget_used_usd
  const budget = progress?.budget_usd ?? mission.budget_usd
  // Percentage only when the budget is a real positive number — a $0.00
  // budget with measured spend has no meaningful percentage (never fabricate
  // one; the line still shows the raw amounts).
  const usagePct = usage != null && budget != null && budget > 0
    ? Math.round((usage / budget) * 100)
    : null

  const taskIdOf = (row: MissionTaskProgress) => `mission-task-row-${row.task_id}`

  return (
    <div
      className="bg-surface-container-lowest rounded-2xl p-xl mb-lg border border-outline-variant/30 relative shadow-e1 aurora-line"
      data-testid="mission-card"
      aria-label={t('opc.missionCard.cardAria')}
    >
      <div className="flex items-start gap-md">
        <span className="material-symbols-outlined text-primary mt-xs" style={{ fontSize: 30 }} aria-hidden="true">
          flag
        </span>
        <div className="flex-1 min-w-0">
          <div className="flex items-center justify-between mb-sm">
            <div className="flex items-center gap-sm uppercase font-label-md text-label-sm tracking-widest text-on-surface-variant font-bold">
              <span className="w-1.5 h-1.5 bg-outline-variant rotate-45 block" />
              {t('opc.missionCard.title')}
            </div>
            <Button
              variant="link"
              size="sm"
              className="text-label-sm h-auto px-0 hover:underline"
              onClick={() => (editing ? setEditing(false) : startEdit())}
              aria-expanded={editing}
              data-testid="mission-edit-toggle"
            >
              {editing ? t('opc.missionFocus.cancel') : t('opc.missionFocus.edit')}
            </Button>
          </div>

          {editing ? (
            <div className="mt-sm space-y-md" data-testid="mission-edit-form">
              <label className="block">
                <span className="font-label-sm text-label-sm text-on-surface-variant block mb-xs">
                  {t('opc.missionCard.edit.name')}
                </span>
                <input
                  type="text"
                  value={form.name}
                  maxLength={MAX_NAME_LEN + 1}
                  onChange={e => setForm(f => ({ ...f, name: e.target.value }))}
                  aria-invalid={errors.name ? true : undefined}
                  data-testid="mission-edit-name"
                  className="w-full bg-surface-container-low rounded-lg border border-outline-variant/30 px-sm py-sm text-body-sm focus:outline-none focus:ring-2 focus:ring-primary/30"
                />
                {errors.name && (
                  <span className="font-label-sm text-label-xs text-error flex items-center gap-xs mt-xs" role="alert">
                    <span className="material-symbols-outlined icon-sm" aria-hidden="true">error</span>
                    {t(errors.name)}
                  </span>
                )}
              </label>
              <div className="grid grid-cols-1 sm:grid-cols-2 gap-md">
                <label className="block">
                  <span className="font-label-sm text-label-sm text-on-surface-variant block mb-xs">
                    {t('opc.missionCard.edit.budget')}
                  </span>
                  <input
                    type="number"
                    min={0}
                    step="any"
                    value={form.budget}
                    onChange={e => setForm(f => ({ ...f, budget: e.target.value }))}
                    aria-invalid={errors.budget ? true : undefined}
                    data-testid="mission-edit-budget"
                    className="w-full bg-surface-container-low rounded-lg border border-outline-variant/30 px-sm py-sm text-body-sm focus:outline-none focus:ring-2 focus:ring-primary/30"
                  />
                  {errors.budget && (
                    <span className="font-label-sm text-label-xs text-error flex items-center gap-xs mt-xs" role="alert">
                      <span className="material-symbols-outlined icon-sm" aria-hidden="true">error</span>
                      {t(errors.budget)}
                    </span>
                  )}
                </label>
                <label className="block">
                  <span className="font-label-sm text-label-sm text-on-surface-variant block mb-xs">
                    {t('opc.missionCard.edit.deadline')}
                  </span>
                  <input
                    type="date"
                    value={form.deadline}
                    onChange={e => setForm(f => ({ ...f, deadline: e.target.value }))}
                    aria-invalid={errors.deadline ? true : undefined}
                    data-testid="mission-edit-deadline"
                    className="w-full bg-surface-container-low rounded-lg border border-outline-variant/30 px-sm py-sm text-body-sm focus:outline-none focus:ring-2 focus:ring-primary/30"
                  />
                  {errors.deadline && (
                    <span className="font-label-sm text-label-xs text-error flex items-center gap-xs mt-xs" role="alert">
                      <span className="material-symbols-outlined icon-sm" aria-hidden="true">error</span>
                      {t(errors.deadline)}
                    </span>
                  )}
                </label>
              </div>
              <div>
                <span className="font-label-sm text-label-sm text-on-surface-variant block mb-xs">
                  {t('opc.missionCard.edit.tasks')}
                </span>
                {tasks.length > 0 ? (
                  <div className="flex flex-wrap gap-sm" data-testid="mission-edit-tasks">
                    {tasks.map(task => (
                      <label
                        key={task.id}
                        className="inline-flex items-center gap-xs px-sm py-xs rounded-full bg-surface-container-low text-label-sm cursor-pointer max-w-full"
                      >
                        <input
                          type="checkbox"
                          className="accent-primary shrink-0"
                          checked={form.taskIds.includes(task.id)}
                          onChange={(e) => {
                            setForm(f => ({
                              ...f,
                              taskIds: e.target.checked
                                ? [...f.taskIds, task.id]
                                : f.taskIds.filter(id => id !== task.id),
                            }))
                          }}
                          data-testid={`mission-edit-task-${task.id}`}
                        />
                        <span className="truncate">{task.title}</span>
                      </label>
                    ))}
                  </div>
                ) : null}
                {offBoardIds.length > 0 && (
                  <div className="flex flex-wrap gap-sm mt-sm">
                    {offBoardIds.map(id => (
                      <span
                        key={id}
                        className="inline-flex items-center gap-xs px-sm py-xs rounded-full bg-surface-container-low text-label-sm text-on-surface-variant max-w-full"
                      >
                        <span className="truncate font-mono">{id}</span>
                        <button
                          type="button"
                          className="cursor-pointer hover:text-error"
                          aria-label={t('opc.missionCard.edit.unlink', { taskId: id })}
                          data-testid={`mission-edit-unlink-${id}`}
                          onClick={() => setForm(f => ({ ...f, taskIds: f.taskIds.filter(x => x !== id) }))}
                        >
                          <span className="material-symbols-outlined icon-sm" aria-hidden="true">close</span>
                        </button>
                      </span>
                    ))}
                  </div>
                )}
                {errors.taskIds && (
                  <span className="font-label-sm text-label-xs text-error flex items-center gap-xs mt-xs" role="alert">
                    <span className="material-symbols-outlined icon-sm" aria-hidden="true">error</span>
                    {t(errors.taskIds)}
                  </span>
                )}
              </div>
              <Button
                className="px-md py-sm rounded-lg font-label-md hover:opacity-90"
                onClick={save}
                disabled={saving || Object.keys(errors).length > 0}
                data-testid="mission-edit-save"
              >
                {t('opc.missionCard.save')}
              </Button>
            </div>
          ) : (
            <>
              <h2 className="font-headline-lg text-[28px] font-bold text-on-surface mt-sm max-w-5xl break-words">
                {mission.name}
              </h2>
              {/* Budget / deadline chips — each renders ONLY when the config
                  carries the field (absent stays absent). */}
              {(mission.budget_usd != null || mission.deadline_ts != null) && (
                <div className="flex items-center gap-md mt-sm flex-wrap font-label-md text-label-md text-on-surface-variant">
                  {mission.budget_usd != null && (
                    <span className="inline-flex items-center gap-xs tabular-nums" data-testid="mission-budget">
                      <span className="material-symbols-outlined icon-sm" aria-hidden="true">payments</span>
                      {fmtUsd(mission.budget_usd)}
                    </span>
                  )}
                  {mission.deadline_ts != null && (
                    <span className="inline-flex items-center gap-xs" data-testid="mission-deadline">
                      <span className="material-symbols-outlined icon-sm" aria-hidden="true">event</span>
                      <span className="tabular-nums">
                        {intl.formatDate(mission.deadline_ts, { year: 'numeric', month: 'short', day: 'numeric' })}
                      </span>
                      {overdue && (
                        <span
                          className="text-label-sm text-on-surface-variant/70"
                          data-testid="mission-overdue"
                        >
                          · {t('opc.missionCard.overdue')}
                        </span>
                      )}
                    </span>
                  )}
                </div>
              )}
            </>
          )}

          {/* Linked-task progress — from the mission_progress projection.
              Identity (name/budget/deadline) is config-driven above; this
              area is the only part that needs the read model. Visible in
              both display and edit states, like the hero's progress strip. */}
          <div
            className="mt-md"
            role="status"
            aria-label={t('opc.missionCard.progress.aria')}
            data-testid="mission-progress"
          >
            {progressLoading && (
              <p className="font-label-sm text-label-sm text-on-surface-variant" data-testid="mission-progress-loading">
                {t('opc.missionCard.progress.loading')}
              </p>
            )}
            {progressFailed && (
              <p className="font-label-sm text-label-sm text-error flex items-center gap-xs" role="alert" data-testid="mission-progress-error">
                <span className="material-symbols-outlined icon-sm" aria-hidden="true">error</span>
                {t('opc.missionCard.progress.error')}
              </p>
            )}
            {!progressLoading && !progressFailed && progress && (
              <>
                {/* Budget usage — only when the ledger actually measured
                    spend AND a budget exists; otherwise nothing at all. */}
                {usage != null && budget != null && (
                  <div className="mb-sm" data-testid="mission-budget-usage">
                    <div className="h-1.5 w-full max-w-xl bg-surface-container rounded-full overflow-hidden">
                      <div
                        className="h-full bg-primary rounded-full transition-all duration-(--duration-slower)"
                        style={{ width: `${Math.min(100, Math.max(0, usagePct ?? 0))}%` }}
                      />
                    </div>
                    <p className="font-label-sm text-label-sm text-on-surface-variant mt-xs tabular-nums">
                      {usagePct != null
                        ? t('opc.missionCard.budgetUsage', {
                          used: fmtUsd(usage),
                          budget: fmtUsd(budget ?? 0),
                          percent: usagePct,
                        })
                        : t('opc.missionCard.budgetUsageNoPct', {
                          used: fmtUsd(usage ?? 0),
                          budget: fmtUsd(budget ?? 0),
                        })}
                    </p>
                  </div>
                )}
                {progress.tasks.length === 0 ? (
                  <p className="font-label-sm text-label-sm text-on-surface-variant">
                    {t('opc.missionCard.tasksEmpty')}
                  </p>
                ) : (
                  <ul className="divide-y divide-outline-variant/20">
                    {progress.tasks.map(row => (
                      <li key={row.task_id} className="py-xs flex items-center gap-md min-w-0" data-testid={taskIdOf(row)}>
                        <span className="flex-1 min-w-0 truncate font-body-sm text-on-surface">
                          {row.found ? (row.title ?? row.task_id) : (
                            <span className="inline-flex items-center gap-xs text-on-surface-variant italic">
                              <span className="material-symbols-outlined icon-sm" aria-hidden="true">help_outline</span>
                              {t('opc.missionCard.taskMissing')}
                              <span className="font-mono text-label-xs">({row.task_id})</span>
                            </span>
                          )}
                        </span>
                        {row.found && row.status && (
                          <span className="inline-flex items-center gap-xs font-label-sm text-label-sm text-on-surface-variant shrink-0">
                            <span className={`w-1.5 h-1.5 rounded-full block ${STATUS_FAMILY[classifyStatus(row.status)].dotClass}`} aria-hidden="true" />
                            {t(STATUS_FAMILY[classifyStatus(row.status)].titleKey)}
                          </span>
                        )}
                        {row.cost_usd != null && (
                          <span className="font-label-sm text-label-sm text-on-surface-variant tabular-nums shrink-0">
                            {fmtUsd(row.cost_usd)}
                          </span>
                        )}
                      </li>
                    ))}
                  </ul>
                )}
              </>
            )}
          </div>
        </div>
      </div>
    </div>
  )
}
