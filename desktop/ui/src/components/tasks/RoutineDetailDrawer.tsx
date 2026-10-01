// RoutineDetailDrawer — Phase D C4 deliverable.
//
// Right-side drawer for inspecting and editing a scheduled routine.
// P1-1 (G2b): the name/prompt static blocks became the RoutineBasicsEditor
// (name + prompt + trigger editing via update_scheduled_task) and the
// lifecycle row (pause/resume switch + guarded delete) joined the top of
// the body — the "can build but can't stop" gap.
//
// T1.2 — migrated onto the shared <SidePanel> primitive. The previous
// hand-rolled overlay, document-level Escape listener, backdrop click
// handler, and focus trap hook are all gone: SidePanel owns them.
// The `aria-label` interpolates the routine name; that wiring lives
// on SidePanel's `ariaLabel` prop.

import { useIntl } from 'react-intl'
import type { ScheduledRoutine } from '@/types'
import DependsOnEditor from './DependsOnEditor'
import OffpeakWindowEditor from './OffpeakWindowEditor'
import RoutineBasicsEditor from './RoutineBasicsEditor'
import RoutineLifecycleRow from './RoutineLifecycleRow'
import { SidePanel, SidePanelBody, SidePanelCloseButton, SidePanelHeader, SidePanelTitle } from '@/components/ui/side-panel'

interface RoutineDetailDrawerProps {
  routine: ScheduledRoutine | null
  routines: ScheduledRoutine[]
  onClose: () => void
  onUpdated?: (routine: ScheduledRoutine) => void
}

function formatTimestamp(ts?: number | null): string {
  if (!ts) return '—'
  return new Date(ts * 1000).toLocaleString()
}

export default function RoutineDetailDrawer({
  routine,
  routines,
  onClose,
  onUpdated,
}: RoutineDetailDrawerProps) {
  const intl = useIntl()
  const t = (id: string, values?: Record<string, string | number>) =>
    intl.formatMessage({ id }, values)

  if (!routine) return null
  const deps = (routine.depends_on ?? []).map(id => routines.find(r => r.id === id)?.name ?? id)
  const ariaLabel = t('tasks.routineDetailDrawer.ariaLabel', { name: routine.name })
  const closeAria = t('tasks.routineDetailDrawer.closeAria')

  return (
    <SidePanel
      open={!!routine}
      onClose={onClose}
      ariaLabel={ariaLabel}
      width="440px"
    >
      <SidePanelHeader>
        <SidePanelTitle>{t('tasks.routineDetailDrawer.title')}</SidePanelTitle>
        <SidePanelCloseButton onClick={onClose} label={closeAria} />
      </SidePanelHeader>
      <SidePanelBody>
        <div className="space-y-md">
          {/* P1-1: pause/resume switch + guarded delete. The drawer-level
              onUpdated carries the routine; the sub-editors only need a
              "something changed, refresh" signal. */}
          <RoutineLifecycleRow
            key={`lifecycle-${routine.id}`}
            routine={routine}
            onUpdated={onUpdated ? () => onUpdated(routine) : undefined}
            onDeleted={onClose}
          />
          {/* P1-1: name / prompt / trigger editing. Keyed by routine id so
              the fields reset when another routine is opened. */}
          <RoutineBasicsEditor
            key={`basics-${routine.id}`}
            routine={routine}
            onUpdated={onUpdated ? () => onUpdated(routine) : undefined}
          />
          <div className="grid grid-cols-2 gap-md">
            <div>
              <span className="text-label-sm text-on-surface-variant">{t('tasks.routineDetailDrawer.enabled')}</span>
              <p className="font-body-md text-on-surface mt-xs">
                {routine.enabled ? t('tasks.routineDetailDrawer.yes') : t('tasks.routineDetailDrawer.no')}
              </p>
            </div>
            <div>
              <span className="text-label-sm text-on-surface-variant">{t('tasks.routineDetailDrawer.trigger')}</span>
              <p className="font-body-md text-on-surface mt-xs capitalize">
                {routine.trigger_type.charAt(0).toUpperCase() + routine.trigger_type.slice(1)}
              </p>
            </div>
            <div>
              <span className="text-label-sm text-on-surface-variant">{t('tasks.routineDetailDrawer.nextFire')}</span>
              <p className="font-body-md text-on-surface mt-xs">{formatTimestamp(routine.next_fire_at)}</p>
            </div>
            <div>
              <span className="text-label-sm text-on-surface-variant">{t('tasks.routineDetailDrawer.lastFire')}</span>
              <p className="font-body-md text-on-surface mt-xs">{formatTimestamp(routine.last_fired)}</p>
            </div>
          </div>
          {routine.last_error && (
            <div className="rounded-xl border border-error/20 bg-error/10 px-md py-sm">
              <span className="text-label-sm text-error">{t('tasks.routineDetailDrawer.lastError')}</span>
              <p className="font-body-md text-error mt-xs break-words">{routine.last_error}</p>
            </div>
          )}
          {/* P2-5: off-peak execution window editor + queued status.
              Keyed by routine id so toggles/inputs reset per routine. */}
          <OffpeakWindowEditor key={routine.id} routine={routine} onUpdated={onUpdated} />
          <div>
            <div className="flex items-center justify-between mb-sm">
              <span className="text-label-sm text-on-surface-variant uppercase tracking-wider">
                {t('tasks.routineDetailDrawer.dependencies')}
              </span>
              <span className="font-label-sm text-label-xs text-on-surface-variant">
                {deps.length === 0 ? t('tasks.routineDetailDrawer.none') : deps.join(', ')}
              </span>
            </div>
            <DependsOnEditor routine={routine} routines={routines} onUpdated={onUpdated} />
          </div>
        </div>
      </SidePanelBody>
    </SidePanel>
  )
}