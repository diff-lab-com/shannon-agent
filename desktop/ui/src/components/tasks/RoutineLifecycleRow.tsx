// RoutineLifecycleRow — P1-1 (G2b): pause/resume + delete for a routine,
// inline in the detail drawer.
//
// - The switch calls toggle_scheduled_task with the EXPLICIT target state
//   and keys its toast off the persisted bool the backend returns (read-back
//   fool-proofing) — a flip-only contract would race a stale list and
//   silently invert the user's click.
// - Delete is guarded by a destructive ConfirmDialog; the confirm button is
//   busy-locked while the delete is in flight and a failure keeps the
//   drawer (and the routine) untouched.
// - Both controls are disabled while their call is in flight (anti
//   double-click), and the parent's onUpdated refresh re-renders the row
//   from the persisted list state.

import { useState } from 'react'
import { useIntl } from 'react-intl'
import { toast } from 'sonner'
import { toastError } from '@/lib/errorToast'
import { Button } from '@/components/ui/button'
import { ConfirmDialog } from '@/components/ui/confirm-dialog'
import * as api from '@/lib/tauri-api'
import type { ScheduledRoutine } from '@/types'

interface RoutineLifecycleRowProps {
  routine: ScheduledRoutine
  /** Fired after a successful toggle or delete (parent refreshes the list). */
  onUpdated?: () => void
  /** Fired after a successful delete (parent closes the drawer). */
  onDeleted?: () => void
}

export default function RoutineLifecycleRow({ routine, onUpdated, onDeleted }: RoutineLifecycleRowProps) {
  const intl = useIntl()
  const t = (id: string, values?: Record<string, string | number>) =>
    intl.formatMessage({ id }, values)

  const [toggling, setToggling] = useState(false)
  const [confirmOpen, setConfirmOpen] = useState(false)
  const [deleting, setDeleting] = useState(false)

  const nextEnabled = !routine.enabled

  const handleToggle = async () => {
    if (toggling) return
    setToggling(true)
    try {
      const persisted = await api.toggleScheduledTask(routine.id, nextEnabled)
      toast.success(t(persisted ? 'tasks.toast.enabled' : 'tasks.toast.disabled'))
      onUpdated?.()
    } catch (e) {
      toastError(t('tasks.toast.failed.toggle'), e)
    } finally {
      setToggling(false)
    }
  }

  const handleDelete = async () => {
    if (deleting) return
    setDeleting(true)
    try {
      await api.deleteScheduledTask(routine.id)
      toast.success(t('tasks.toast.deleted'))
      setConfirmOpen(false)
      onUpdated?.()
      onDeleted?.()
    } catch (e) {
      // Failure keeps the routine — the drawer stays open with its state.
      setConfirmOpen(false)
      toastError(t('tasks.toast.failed.delete'), e)
    } finally {
      setDeleting(false)
    }
  }

  return (
    <div className="rounded-xl border border-outline-variant/20 bg-surface-container-low/60 p-md flex items-center justify-between gap-md">
      <label className="flex items-center gap-sm cursor-pointer">
        <input
          type="checkbox"
          className="w-4 h-4 accent-primary cursor-pointer"
          checked={routine.enabled}
          disabled={toggling}
          onChange={() => void handleToggle()}
          data-testid="routine-lifecycle-toggle"
          aria-label={t('tasks.routineControls.toggleAria')}
        />
        <span
          className={`font-label-md font-semibold flex items-center gap-xs ${
            routine.enabled ? 'text-on-surface' : 'text-on-surface-variant'
          }`}
        >
          <span className="material-symbols-outlined icon-sm" aria-hidden="true">
            {routine.enabled ? 'play_circle' : 'pause_circle'}
          </span>
          {routine.enabled
            ? t('tasks.routineControls.enabledLabel')
            : t('tasks.routineControls.pausedLabel')}
        </span>
      </label>

      <Button
        variant="outline"
        type="button"
        size="sm"
        onClick={() => setConfirmOpen(true)}
        data-testid="routine-lifecycle-delete"
        aria-label={t('tasks.routineControls.deleteAria')}
        className="rounded-lg border-error/40 text-error hover:bg-error/10"
      >
        <span className="material-symbols-outlined icon-sm" aria-hidden="true">delete</span>
        {t('tasks.routineControls.delete')}
      </Button>

      <ConfirmDialog
        open={confirmOpen}
        title={t('tasks.routineControls.deleteTitle')}
        message={t('tasks.routineControls.deleteMessage', { name: routine.name })}
        confirmLabel={t('tasks.routineControls.deleteConfirm')}
        cancelLabel={t('tasks.routineControls.cancel')}
        destructive
        busy={deleting}
        busyLabel={t('tasks.routineControls.deleteBusy')}
        onConfirm={() => void handleDelete()}
        onCancel={() => { if (!deleting) setConfirmOpen(false) }}
      />
    </div>
  )
}
