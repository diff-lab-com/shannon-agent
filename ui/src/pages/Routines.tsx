import { useState, useEffect } from 'react'
import { toast } from 'sonner'
import * as api from '@/lib/tauri-api'
import type { TriggeredRoutineDto } from '@/types'

const TRIGGER_OPTIONS = [
  'PostToolUse',
  'PreToolUse',
  'SubagentStart',
  'SubagentStop',
  'SessionStart',
  'SessionEnd',
  'PreCompact',
  'PostCompact',
  'TaskCreated',
  'TaskCompleted',
  'WorktreeCreate',
  'WorktreeRemove',
  'ConfigChange',
  'InstructionsLoaded',
]

interface FormState {
  name: string
  trigger: string
  command: string
  matcher: string
  pattern: string
  description: string
}

const EMPTY_FORM: FormState = {
  name: '',
  trigger: 'PostToolUse',
  command: '',
  matcher: '',
  pattern: '',
  description: '',
}

export default function Routines() {
  const [routines, setRoutines] = useState<TriggeredRoutineDto[]>([])
  const [loading, setLoading] = useState(true)
  const [showCreate, setShowCreate] = useState(false)
  const [form, setForm] = useState<FormState>(EMPTY_FORM)
  const [saving, setSaving] = useState(false)

  const load = async () => {
    try {
      setRoutines(await api.listTriggeredRoutines())
    } catch (e) {
      console.warn('Failed to load routines:', e)
      toast.error('Failed to load routines')
    }
    setLoading(false)
  }

  useEffect(() => { load() }, [])

  const handleToggle = async (name: string, enabled: boolean) => {
    const prev = routines
    setRoutines(rs => rs.map(r => r.name === name ? { ...r, enabled } : r))
    try {
      await api.toggleTriggeredRoutine(name, enabled)
      toast.success(`${name} ${enabled ? 'enabled' : 'disabled'}`)
    } catch (e) {
      console.warn('Failed to toggle routine:', e)
      setRoutines(prev)
      toast.error('Failed to toggle')
    }
  }

  const handleCreate = async () => {
    if (!form.name.trim() || !form.command.trim()) {
      toast.error('Name and command are required')
      return
    }
    setSaving(true)
    try {
      await api.createTriggeredRoutine({
        name: form.name.trim(),
        trigger: form.trigger,
        command: form.command.trim(),
        matcher: form.matcher.trim() || undefined,
        pattern: form.pattern.trim() || undefined,
        description: form.description.trim() || undefined,
      })
      toast.success(`Created "${form.name.trim()}"`)
      setForm(EMPTY_FORM)
      setShowCreate(false)
      await load()
    } catch (e) {
      console.warn('Failed to create routine:', e)
      toast.error('Failed to create routine')
    }
    setSaving(false)
  }

  const enabledCount = routines.filter(r => r.enabled).length

  return (
    <div className="p-xl space-y-lg max-w-4xl">
      <header className="flex items-start justify-between gap-md">
        <div>
          <h1 className="font-headline-lg text-on-surface mb-xs">Routines</h1>
          <p className="font-body-md text-on-surface-variant">
            Triggered routines fire automatically on Shannon events (PostToolUse, PreCompact, WorktreeCreate, …). Scheduled routines live in <code className="font-mono bg-surface-container-high px-xs rounded text-[12px]">/tasks</code>.
          </p>
        </div>
        <button
          onClick={() => setShowCreate(s => !s)}
          aria-expanded={showCreate}
          className="px-lg py-sm bg-primary text-on-primary rounded-lg font-label-md cursor-pointer hover:bg-primary/90 transition-colors flex items-center gap-sm shrink-0 focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-primary"
        >
          <span className="material-symbols-outlined text-[18px]">{showCreate ? 'close' : 'add'}</span>
          {showCreate ? 'Cancel' : 'New Routine'}
        </button>
      </header>

      {showCreate && (
        <CreateForm form={form} setForm={setForm} onSave={handleCreate} saving={saving} onCancel={() => setShowCreate(false)} />
      )}

      {loading ? (
        <div className="flex items-center justify-center py-xl">
          <span className="material-symbols-outlined text-[32px] text-primary animate-spin">progress_activity</span>
        </div>
      ) : routines.length === 0 ? (
        <div className="bg-surface-container-lowest border border-outline-variant/30 rounded-xl p-xl text-center">
          <span className="material-symbols-outlined text-[48px] text-outline-variant block mb-sm">bolt</span>
          <p className="font-headline-md text-on-surface mb-xs">No routines yet</p>
          <p className="font-body-sm text-on-surface-variant">Create one to automate a response to a Shannon event.</p>
        </div>
      ) : (
        <>
          <div className="font-label-sm text-on-surface-variant">
            {routines.length} routine{routines.length !== 1 ? 's' : ''} · {enabledCount} enabled
          </div>
          <div className="space-y-sm">
            {routines.map(r => (
              <RoutineRow key={r.name} routine={r} onToggle={handleToggle} />
            ))}
          </div>
        </>
      )}
    </div>
  )
}

function RoutineRow({ routine, onToggle }: { routine: TriggeredRoutineDto; onToggle: (name: string, enabled: boolean) => void }) {
  return (
    <div className="bg-surface-container-lowest border border-outline-variant/30 rounded-xl p-md flex items-start justify-between gap-md">
      <div className="flex-1 min-w-0">
        <div className="flex items-center gap-sm mb-xs">
          <span className="font-headline-md text-on-surface truncate">{routine.name}</span>
          <span className="px-xs py-[2px] bg-primary-container/40 text-primary rounded text-[10px] font-mono shrink-0">{routine.trigger}</span>
        </div>
        {routine.description && (
          <p className="font-body-sm text-on-surface-variant mb-xs truncate">{routine.description}</p>
        )}
        <div className="flex items-center gap-md font-label-sm text-on-surface-variant flex-wrap">
          <span className="font-mono text-[12px] truncate max-w-[300px]">
            <span className="material-symbols-outlined text-[14px] align-middle mr-xs">terminal</span>
            {routine.command}
          </span>
          {routine.matcher && (
            <span className="text-[11px]">
              <span className="text-outline">matcher:</span> <span className="font-mono">{routine.matcher}</span>
            </span>
          )}
          {routine.pattern && (
            <span className="text-[11px]">
              <span className="text-outline">pattern:</span> <span className="font-mono">{routine.pattern}</span>
            </span>
          )}
        </div>
      </div>
      <button
        onClick={() => onToggle(routine.name, !routine.enabled)}
        aria-pressed={routine.enabled}
        aria-label={`${routine.enabled ? 'Disable' : 'Enable'} ${routine.name}`}
        className={`relative w-11 h-6 rounded-full transition-colors shrink-0 cursor-pointer focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-primary ${routine.enabled ? 'bg-primary' : 'bg-outline-variant'}`}
      >
        <span
          className={`absolute top-1 left-1 w-4 h-4 rounded-full bg-surface-container-lowest shadow-sm transition-transform ${routine.enabled ? 'translate-x-5' : 'translate-x-0'}`}
        />
      </button>
    </div>
  )
}

function CreateForm({ form, setForm, onSave, onCancel, saving }: {
  form: FormState
  setForm: (f: FormState) => void
  onSave: () => void
  onCancel: () => void
  saving: boolean
}) {
  return (
    <section className="bg-surface-container-lowest border border-outline-variant/30 rounded-xl p-lg space-y-md">
      <h2 className="font-headline-md text-on-surface">New triggered routine</h2>

      <div className="grid grid-cols-1 md:grid-cols-2 gap-md">
        <Field label="Name" required>
          <input
            value={form.name}
            onChange={e => setForm({ ...form, name: e.target.value })}
            placeholder="lint-after-edit"
            className="w-full px-md py-sm bg-surface border border-outline-variant/50 rounded-lg focus:ring-2 focus:ring-primary outline-none font-body-sm font-mono"
          />
        </Field>

        <Field label="Trigger event" required>
          <select
            value={form.trigger}
            onChange={e => setForm({ ...form, trigger: e.target.value })}
            className="w-full px-md py-sm bg-surface border border-outline-variant/50 rounded-lg focus:ring-2 focus:ring-primary outline-none font-body-sm"
          >
            {TRIGGER_OPTIONS.map(t => <option key={t} value={t}>{t}</option>)}
          </select>
        </Field>

        <Field label="Command" required hint="Shell command or Shannon prompt — runs when the trigger fires">
          <input
            value={form.command}
            onChange={e => setForm({ ...form, command: e.target.value })}
            placeholder="pnpm lint"
            className="w-full px-md py-sm bg-surface border border-outline-variant/50 rounded-lg focus:ring-2 focus:ring-primary outline-none font-body-sm font-mono"
          />
        </Field>

        <Field label="Matcher (optional)" hint="Tool name for *ToolUse triggers; otherwise free text">
          <input
            value={form.matcher}
            onChange={e => setForm({ ...form, matcher: e.target.value })}
            placeholder="bash"
            className="w-full px-md py-sm bg-surface border border-outline-variant/50 rounded-lg focus:ring-2 focus:ring-primary outline-none font-body-sm font-mono"
          />
        </Field>

        <Field label="Pattern (optional)" hint="Regex filter on the matcher payload">
          <input
            value={form.pattern}
            onChange={e => setForm({ ...form, pattern: e.target.value })}
            placeholder="\\.py$"
            className="w-full px-md py-sm bg-surface border border-outline-variant/50 rounded-lg focus:ring-2 focus:ring-primary outline-none font-body-sm font-mono"
          />
        </Field>

        <Field label="Description (optional)">
          <input
            value={form.description}
            onChange={e => setForm({ ...form, description: e.target.value })}
            placeholder="Auto-lint Python files after bash edits"
            className="w-full px-md py-sm bg-surface border border-outline-variant/50 rounded-lg focus:ring-2 focus:ring-primary outline-none font-body-sm"
          />
        </Field>
      </div>

      <div className="flex justify-end gap-sm pt-sm">
        <button
          onClick={onCancel}
          disabled={saving}
          className="px-md py-sm text-on-surface-variant hover:text-primary font-label-md cursor-pointer disabled:opacity-50"
        >
          Cancel
        </button>
        <button
          onClick={onSave}
          disabled={saving || !form.name.trim() || !form.command.trim()}
          className="px-lg py-sm bg-primary text-on-primary rounded-lg font-label-md cursor-pointer hover:bg-primary/90 disabled:opacity-50 disabled:cursor-not-allowed flex items-center gap-sm"
        >
          {saving && <span className="material-symbols-outlined text-[16px] animate-spin">progress_activity</span>}
          {saving ? 'Saving…' : 'Create routine'}
        </button>
      </div>
    </section>
  )
}

function Field({ label, required, hint, children }: { label: string; required?: boolean; hint?: string; children: React.ReactNode }) {
  return (
    <label className="block">
      <span className="font-label-md text-on-surface-variant block mb-xs">
        {label}{required && <span className="text-primary ml-xs" aria-hidden="true">*</span>}
      </span>
      {children}
      {hint && <span className="font-label-sm text-outline block mt-xs">{hint}</span>}
    </label>
  )
}
