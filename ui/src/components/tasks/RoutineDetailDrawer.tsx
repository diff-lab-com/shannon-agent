// RoutineDetailDrawer — Phase D C4 deliverable.
//
// Right-side drawer for inspecting and editing a scheduled routine.
// Currently exposes the DependsOnEditor; later phases can add prompt /
// trigger / policy editing here too. Click backdrop or close button to
// dismiss. Escape closes too.

import type { ScheduledRoutine } from '@/types'
import DependsOnEditor from './DependsOnEditor'

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
  if (!routine) return null
  const deps = (routine.depends_on ?? []).map(id => routines.find(r => r.id === id)?.name ?? id)
  return (
    <div
      className="fixed inset-0 z-50 flex justify-end"
      onClick={onClose}
      onKeyDown={e => { if (e.key === 'Escape') onClose() }}
      role="dialog"
      aria-modal="true"
      aria-label={`Routine detail: ${routine.name}`}
    >
      <div className="bg-black/20 absolute inset-0" />
      <div
        className="relative w-[440px] bg-surface-container-lowest shadow-2xl border-l border-outline-variant/20 p-xl overflow-y-auto"
        onClick={e => e.stopPropagation()}
      >
        <div className="flex items-center justify-between mb-lg">
          <h3 className="font-headline-md text-on-surface font-bold">Routine Detail</h3>
          <button
            aria-label="Close drawer"
            className="p-sm rounded-lg hover:bg-surface-container text-on-surface-variant cursor-pointer focus:outline-none focus-visible:ring-2 focus-visible:ring-primary/30"
            onClick={onClose}
          >
            <span className="material-symbols-outlined">close</span>
          </button>
        </div>
        <div className="space-y-md">
          <div>
            <span className="text-label-sm text-on-surface-variant">Name</span>
            <p className="font-body-lg text-on-surface font-bold mt-xs">{routine.name}</p>
          </div>
          <div>
            <span className="text-label-sm text-on-surface-variant">Prompt</span>
            <p className="font-body-md text-on-surface mt-xs whitespace-pre-wrap break-words">
              {routine.prompt}
            </p>
          </div>
          <div className="grid grid-cols-2 gap-md">
            <div>
              <span className="text-label-sm text-on-surface-variant">Trigger</span>
              <p className="font-body-md text-on-surface mt-xs capitalize">
                {routine.trigger_type.charAt(0).toUpperCase() + routine.trigger_type.slice(1)}
              </p>
            </div>
            <div>
              <span className="text-label-sm text-on-surface-variant">Enabled</span>
              <p className="font-body-md text-on-surface mt-xs">{routine.enabled ? 'Yes' : 'No'}</p>
            </div>
            <div>
              <span className="text-label-sm text-on-surface-variant">Next fire</span>
              <p className="font-body-md text-on-surface mt-xs">{formatTimestamp(routine.next_fire_at)}</p>
            </div>
            <div>
              <span className="text-label-sm text-on-surface-variant">Last fire</span>
              <p className="font-body-md text-on-surface mt-xs">{formatTimestamp(routine.last_fired)}</p>
            </div>
          </div>
          {routine.last_error && (
            <div className="rounded-xl border border-error/20 bg-error/10 px-md py-sm">
              <span className="text-label-sm text-error">Last error</span>
              <p className="font-body-md text-error mt-xs break-words">{routine.last_error}</p>
            </div>
          )}
          <div>
            <div className="flex items-center justify-between mb-sm">
              <span className="text-label-sm text-on-surface-variant uppercase tracking-wider">
                Dependencies
              </span>
              <span className="font-label-sm text-[11px] text-on-surface-variant">
                {deps.length === 0 ? 'None' : deps.join(', ')}
              </span>
            </div>
            <DependsOnEditor routine={routine} routines={routines} onUpdated={onUpdated} />
          </div>
        </div>
      </div>
    </div>
  )
}
