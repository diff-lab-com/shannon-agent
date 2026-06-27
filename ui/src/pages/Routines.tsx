import { useState, useEffect } from 'react'
import { useIntl } from 'react-intl'
import { toast } from 'sonner'
import { toastError } from '@/lib/errorToast'
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
  const intl = useIntl()
  const t = (id: string, values?: Record<string, any>) => intl.formatMessage({ id }, values)
  const [routines, setRoutines] = useState<TriggeredRoutineDto[]>([])
  const [loading, setLoading] = useState(true)
  const [showCreate, setShowCreate] = useState(false)
  const [form, setForm] = useState<FormState>(EMPTY_FORM)
  const [saving, setSaving] = useState(false)

  const load = async () => {
    try {
      setRoutines(await api.listTriggeredRoutines())
    } catch (e) {
      toastError(t('routines.error.load'), e)
    }
    setLoading(false)
  }

  useEffect(() => { load() }, [])

  const handleToggle = async (name: string, enabled: boolean) => {
    const prev = routines
    setRoutines(rs => rs.map(r => r.name === name ? { ...r, enabled } : r))
    try {
      await api.toggleTriggeredRoutine(name, enabled)
      toast.success(t(enabled ? 'routines.toast.enabled' : 'routines.toast.disabled', { name }))
    } catch (e) {
      setRoutines(prev)
      toastError(t('routines.error.toggle'), e)
    }
  }

  const handleCreate = async () => {
    if (!form.name.trim() || !form.command.trim()) {
      toast.error(t('routines.error.required'))
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
      toast.success(t('routines.toast.created', { name: form.name.trim() }))
      setForm(EMPTY_FORM)
      setShowCreate(false)
      await load()
    } catch (e) {
      toastError(t('routines.error.create'), e)
    }
    setSaving(false)
  }

  const enabledCount = routines.filter(r => r.enabled).length

  return (
    <div className="p-xl space-y-lg max-w-4xl">
      <header className="flex items-start justify-between gap-md">
        <div>
          <h1 className="font-headline-lg text-on-surface mb-xs">{t('routines.title')}</h1>
          <p className="font-body-md text-on-surface-variant">
            {t('routines.subtitle')}<code className="font-mono bg-surface-container-high px-xs rounded text-[12px]">{t('routines.subtitle.code')}</code>.
          </p>
        </div>
        <button
          onClick={() => setShowCreate(s => !s)}
          aria-expanded={showCreate}
          className="px-lg py-sm bg-primary text-on-primary rounded-lg font-label-md cursor-pointer hover:bg-primary/90 transition-colors flex items-center gap-sm shrink-0 focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-primary"
        >
          <span className="material-symbols-outlined text-[18px]">{showCreate ? 'close' : 'add'}</span>
          {showCreate ? t('routines.cancel') : t('routines.newRoutine')}
        </button>
      </header>

      {showCreate && (
        <CreateForm form={form} setForm={setForm} onSave={handleCreate} saving={saving} onCancel={() => setShowCreate(false)} />
      )}

      {loading ? (
        <div className="flex items-center justify-center py-xl">
          <span className="material-symbols-outlined icon-xl text-primary animate-spin">progress_activity</span>
        </div>
      ) : routines.length === 0 ? (
        <div className="bg-surface-container-lowest border border-outline-variant/30 rounded-xl p-xl text-center">
          <span className="material-symbols-outlined icon-2xl text-outline-variant block mb-sm">bolt</span>
          <p className="font-headline-md text-on-surface mb-xs">{t('routines.empty.title')}</p>
          <p className="font-body-sm text-on-surface-variant">{t('routines.empty.description')}</p>
        </div>
      ) : (
        <>
          <div className="font-label-sm text-on-surface-variant">
            {t('routines.count', { count: routines.length })} · {enabledCount} {t('routines.enabled')}
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
  const intl = useIntl()
  const t = (id: string, values?: any) => intl.formatMessage({ id }, values)
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
        aria-label={t(routine.enabled ? 'routines.toggle.disable' : 'routines.toggle.enable') + ' ' + routine.name}
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
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })

  return (
    <section className="bg-surface-container-lowest border border-outline-variant/30 rounded-xl p-lg space-y-md">
      <h2 className="font-headline-md text-on-surface">{t('routines.new.title')}</h2>

      <div className="grid grid-cols-1 md:grid-cols-2 gap-md">
        <Field label={t('routines.form.name')} required>
          <input
            value={form.name}
            onChange={e => setForm({ ...form, name: e.target.value })}
            placeholder={t('routines.form.name.placeholder')}
            className="w-full px-md py-sm bg-surface border border-outline-variant/50 rounded-lg focus:ring-2 focus:ring-primary outline-none font-body-sm font-mono"
          />
        </Field>

        <Field label={t('routines.form.trigger')} required>
          <select
            value={form.trigger}
            onChange={e => setForm({ ...form, trigger: e.target.value })}
            className="w-full px-md py-sm bg-surface border border-outline-variant/50 rounded-lg focus:ring-2 focus:ring-primary outline-none font-body-sm"
          >
            {TRIGGER_OPTIONS.map(t => <option key={t} value={t}>{t}</option>)}
          </select>
        </Field>

        <Field label={t('routines.form.command')} required hint={t('routines.form.command.hint')}>
          <input
            value={form.command}
            onChange={e => setForm({ ...form, command: e.target.value })}
            placeholder={t('routines.form.command.placeholder')}
            className="w-full px-md py-sm bg-surface border border-outline-variant/50 rounded-lg focus:ring-2 focus:ring-primary outline-none font-body-sm font-mono"
          />
        </Field>

        <Field label={t('routines.form.matcher')} hint={t('routines.form.matcher.hint')}>
          <input
            value={form.matcher}
            onChange={e => setForm({ ...form, matcher: e.target.value })}
            placeholder={t('routines.form.matcher.placeholder')}
            className="w-full px-md py-sm bg-surface border border-outline-variant/50 rounded-lg focus:ring-2 focus:ring-primary outline-none font-body-sm font-mono"
          />
        </Field>

        <Field label={t('routines.form.pattern')} hint={t('routines.form.pattern.hint')}>
          <input
            value={form.pattern}
            onChange={e => setForm({ ...form, pattern: e.target.value })}
            placeholder={t('routines.form.pattern.placeholder')}
            className="w-full px-md py-sm bg-surface border border-outline-variant/50 rounded-lg focus:ring-2 focus:ring-primary outline-none font-body-sm font-mono"
          />
        </Field>

        <Field label={t('routines.form.description')}>
          <input
            value={form.description}
            onChange={e => setForm({ ...form, description: e.target.value })}
            placeholder={t('routines.form.description.placeholder')}
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
          {t('routines.form.cancel')}
        </button>
        <button
          onClick={onSave}
          disabled={saving || !form.name.trim() || !form.command.trim()}
          className="px-lg py-sm bg-primary text-on-primary rounded-lg font-label-md cursor-pointer hover:bg-primary/90 disabled:opacity-50 disabled:cursor-not-allowed flex items-center gap-sm"
        >
          {saving && <span className="material-symbols-outlined icon-sm animate-spin">progress_activity</span>}
          {saving ? t('routines.form.saving') : t('routines.form.create')}
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
