// DependsOnEditor — Phase D C4 deliverable.
//
// Picks which other routines must succeed before this one fires. The list is
// sent wholesale on each toggle (add or remove), matching the
// `update_scheduled_task({ depends_on })` Tauri command which replaces the
// dependency list rather than mutating it.

import { useMemo, useState } from 'react'
import { toast } from 'sonner'
import * as api from '@/lib/tauri-api'
import type { ScheduledRoutine } from '@/types'

interface DependsOnEditorProps {
  routine: ScheduledRoutine
  routines: ScheduledRoutine[]
  onUpdated?: (routine: ScheduledRoutine) => void
}

export default function DependsOnEditor({ routine, routines, onUpdated }: DependsOnEditorProps) {
  const initial = useMemo(() => new Set(routine.depends_on ?? []), [routine.id, routine.depends_on])
  const [selected, setSelected] = useState<Set<string>>(initial)
  const [saving, setSaving] = useState(false)

  const candidates = useMemo(() => {
    return routines
      .filter(r => r.id !== routine.id)
      .sort((a, b) => a.name.localeCompare(b.name))
  }, [routines, routine.id])

  const dirty = useMemo(() => {
    if (selected.size !== initial.size) return true
    for (const id of selected) if (!initial.has(id)) return true
    return false
  }, [selected, initial])

  const toggle = (id: string) => {
    setSelected(prev => {
      const next = new Set(prev)
      if (next.has(id)) next.delete(id)
      else next.add(id)
      return next
    })
  }

  const save = async () => {
    setSaving(true)
    try {
      const updated = await api.updateScheduledTask({
        id: routine.id,
        depends_on: Array.from(selected),
      })
      toast.success('Dependencies updated')
      onUpdated?.(updated)
    } catch (e) {
      const msg = e instanceof Error ? e.message : 'Failed to update dependencies'
      toast.error(msg)
    } finally {
      setSaving(false)
    }
  }

  const reset = () => setSelected(initial)

  if (candidates.length === 0) {
    return (
      <div className="rounded-xl border border-outline-variant/20 bg-surface-container-lowest/60 px-md py-sm text-on-surface-variant font-label-md text-[13px]">
        Create at least one other routine to set dependencies.
      </div>
    )
  }

  return (
    <div className="space-y-sm">
      <div className="rounded-xl border border-outline-variant/20 bg-surface-container-lowest/60 divide-y divide-outline-variant/10">
        {candidates.map(r => {
          const checked = selected.has(r.id)
          return (
            <label
              key={r.id}
              className="flex items-center gap-md px-md py-sm cursor-pointer hover:bg-surface-container-low/40 transition-colors"
            >
              <input
                type="checkbox"
                className="w-4 h-4 accent-primary cursor-pointer"
                checked={checked}
                onChange={() => toggle(r.id)}
                aria-label={`Depends on ${r.name}`}
              />
              <div className="flex-1 min-w-0">
                <div className="font-label-md text-[13px] text-on-surface truncate">{r.name}</div>
                <div className="font-label-sm text-[11px] text-on-surface-variant uppercase tracking-wider">
                  {r.trigger_type}
                </div>
              </div>
            </label>
          )
        })}
      </div>
      <div className="flex items-center gap-sm">
        <button
          type="button"
          className="px-md py-sm bg-primary text-on-primary rounded-xl flex items-center gap-sm font-label-md cursor-pointer hover:shadow-md active:scale-95 transition-all disabled:opacity-50 disabled:cursor-not-allowed focus:outline-none focus-visible:ring-2 focus-visible:ring-primary/30"
          onClick={save}
          disabled={!dirty || saving}
          aria-label="Save dependencies"
        >
          <span className="material-symbols-outlined text-[18px]">save</span>
          {saving ? 'Saving…' : 'Save'}
        </button>
        <button
          type="button"
          className="px-md py-sm border border-outline-variant text-on-surface rounded-xl font-label-md cursor-pointer hover:bg-surface-container-low/40 transition-colors disabled:opacity-50 disabled:cursor-not-allowed focus:outline-none focus-visible:ring-2 focus-visible:ring-primary/30"
          onClick={reset}
          disabled={!dirty || saving}
          aria-label="Discard dependency changes"
        >
          Reset
        </button>
        {selected.size > 0 && (
          <span className="ml-auto font-label-sm text-[12px] text-on-surface-variant">
            {selected.size} selected
          </span>
        )}
      </div>
    </div>
  )
}
