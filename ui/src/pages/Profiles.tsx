import { useState, useEffect } from 'react'
import { toast } from 'sonner'
import * as api from '@/lib/tauri-api'
import type { BuiltinProfileInfo, CustomProfileInfo } from '@/types'

const TOOL_SUGGESTIONS = [
  'Read',
  'Glob',
  'Grep',
  'LS',
  'Bash',
  'Edit',
  'Write',
  'MultiEdit',
  'WebFetch',
  'WebSearch',
]

const AUTO_APPROVE_LABELS: { key: keyof BuiltinProfileInfo; label: string }[] = [
  { key: 'auto_approve_read', label: 'Read' },
  { key: 'auto_approve_write', label: 'Write' },
  { key: 'auto_approve_bash', label: 'Bash' },
  { key: 'auto_approve_delete', label: 'Delete' },
  { key: 'auto_approve_network', label: 'Network' },
]

interface FormState {
  name: string
  description: string
  auto_approve: string
  confirm: string
  deny: string
}

const EMPTY_FORM: FormState = {
  name: '',
  description: '',
  auto_approve: 'Read, Glob, Grep, LS',
  confirm: '',
  deny: '',
}

function parseList(s: string): string[] {
  return s
    .split(/[,\n]/)
    .map(t => t.trim())
    .filter(Boolean)
}

export default function Profiles() {
  const [builtin, setBuiltin] = useState<BuiltinProfileInfo[]>([])
  const [custom, setCustom] = useState<CustomProfileInfo[]>([])
  const [loading, setLoading] = useState(true)
  const [showCreate, setShowCreate] = useState(false)
  const [form, setForm] = useState<FormState>(EMPTY_FORM)
  const [saving, setSaving] = useState(false)

  const load = async () => {
    try {
      const data = await api.listPermissionProfiles()
      setBuiltin(data.builtin)
      setCustom(data.custom)
    } catch (e) {
      console.warn('Failed to load profiles:', e)
      toast.error('Failed to load profiles')
    }
    setLoading(false)
  }

  useEffect(() => { load() }, [])

  const handleSave = async () => {
    const name = form.name.trim()
    if (!name) {
      toast.error('Name is required')
      return
    }
    setSaving(true)
    try {
      await api.saveCustomProfile({
        name,
        description: form.description.trim() || undefined,
        auto_approve: parseList(form.auto_approve),
        confirm: parseList(form.confirm),
        deny: parseList(form.deny),
      })
      toast.success(`Saved "${name}"`)
      setForm(EMPTY_FORM)
      setShowCreate(false)
      await load()
    } catch (e) {
      console.warn('Failed to save profile:', e)
      toast.error(typeof e === 'string' ? e : 'Failed to save profile')
    }
    setSaving(false)
  }

  const handleDelete = async (name: string) => {
    if (!confirm(`Delete profile "${name}"?`)) return
    try {
      await api.deleteCustomProfile(name)
      toast.success(`Deleted "${name}"`)
      await load()
    } catch (e) {
      console.warn('Failed to delete profile:', e)
      toast.error('Failed to delete')
    }
  }

  return (
    <div className="p-xl space-y-lg max-w-4xl">
      <header className="flex items-start justify-between gap-md">
        <div>
          <h1 className="font-headline-lg text-on-surface mb-xs">Permission Profiles</h1>
          <p className="font-body-md text-on-surface-variant">
            Switch profiles via the{' '}
            <code className="font-mono bg-surface-container-high px-xs rounded text-[12px]">/profile</code>{' '}
            command in chat. Custom profiles are loaded from{' '}
            <code className="font-mono bg-surface-container-high px-xs rounded text-[12px]">.shannon/profiles/</code>.
          </p>
        </div>
        <button
          onClick={() => setShowCreate(s => !s)}
          aria-expanded={showCreate}
          className="px-lg py-sm bg-primary text-on-primary rounded-lg font-label-md cursor-pointer hover:bg-primary/90 transition-colors flex items-center gap-sm shrink-0 focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-primary"
        >
          <span className="material-symbols-outlined text-[18px]">{showCreate ? 'close' : 'add'}</span>
          {showCreate ? 'Cancel' : 'New Profile'}
        </button>
      </header>

      {showCreate && (
        <CreateForm form={form} setForm={setForm} onSave={handleSave} saving={saving} onCancel={() => setShowCreate(false)} />
      )}

      {loading ? (
        <div className="flex items-center justify-center py-xl">
          <span className="material-symbols-outlined text-[32px] text-primary animate-spin">progress_activity</span>
        </div>
      ) : (
        <>
          <section className="space-y-sm">
            <h2 className="font-headline-md text-on-surface">Built-in</h2>
            <div className="grid grid-cols-1 md:grid-cols-3 gap-sm">
              {builtin.map(p => (
                <article key={p.id} className="bg-surface-container-lowest border border-outline-variant/30 rounded-xl p-md">
                  <header className="flex items-center justify-between gap-xs mb-xs">
                    <code className="font-mono font-headline-md text-on-surface capitalize">{p.id}</code>
                  </header>
                  <p className="font-body-sm text-on-surface-variant mb-sm">{p.description}</p>
                  <div className="flex items-center gap-xs flex-wrap">
                    {AUTO_APPROVE_LABELS.map(({ key, label }) => (
                      <span
                        key={key}
                        title={`${label}: ${p[key] ? 'auto' : 'needs approval'}`}
                        className={`text-[10px] font-mono px-xs py-[2px] rounded ${
                          p[key]
                            ? 'bg-primary-container/40 text-primary'
                            : 'bg-surface-container-high text-outline'
                        }`}
                      >
                        {label}
                      </span>
                    ))}
                  </div>
                  {p.deny_destructive.length > 0 && (
                    <p className="text-[11px] text-outline mt-xs">
                      Denies: <span className="font-mono">{p.deny_destructive.join(', ')}</span>
                    </p>
                  )}
                </article>
              ))}
            </div>
          </section>

          <section className="space-y-sm">
            <div className="flex items-center justify-between">
              <h2 className="font-headline-md text-on-surface">Custom</h2>
              <span className="font-label-sm text-on-surface-variant">{custom.length} profile{custom.length !== 1 ? 's' : ''}</span>
            </div>
            {custom.length === 0 ? (
              <div className="bg-surface-container-lowest border border-outline-variant/30 rounded-xl p-xl text-center">
                <span className="material-symbols-outlined text-[48px] text-outline-variant block mb-sm">person_off</span>
                <p className="font-headline-md text-on-surface mb-xs">No custom profiles</p>
                <p className="font-body-sm text-on-surface-variant">Create one to bundle tool permission rules by name.</p>
              </div>
            ) : (
              <div className="space-y-sm">
                {custom.map(p => (
                  <ProfileRow key={p.name} profile={p} onDelete={handleDelete} />
                ))}
              </div>
            )}
          </section>
        </>
      )}
    </div>
  )
}

function ProfileRow({ profile, onDelete }: { profile: CustomProfileInfo; onDelete: (name: string) => void }) {
  return (
    <div className="bg-surface-container-lowest border border-outline-variant/30 rounded-xl p-md">
      <div className="flex items-start justify-between gap-md mb-xs">
        <div className="flex-1 min-w-0">
          <code className="font-mono font-headline-md text-on-surface">{profile.name}</code>
          {profile.description && (
            <p className="font-body-sm text-on-surface-variant mt-xs">{profile.description}</p>
          )}
        </div>
        <button
          onClick={() => onDelete(profile.name)}
          aria-label={`Delete ${profile.name}`}
          className="p-xs rounded-lg text-on-surface-variant hover:text-error hover:bg-error-container/30 cursor-pointer transition-colors focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-primary"
        >
          <span className="material-symbols-outlined text-[20px]">delete</span>
        </button>
      </div>
      <div className="flex flex-wrap gap-md font-label-sm">
        <RuleChip label="Auto" tools={profile.auto_approve} tone="approve" />
        <RuleChip label="Confirm" tools={profile.confirm} tone="confirm" />
        <RuleChip label="Deny" tools={profile.deny} tone="deny" />
      </div>
    </div>
  )
}

function RuleChip({ label, tools, tone }: { label: string; tools: string[]; tone: 'approve' | 'confirm' | 'deny' }) {
  if (tools.length === 0) return null
  const toneClass = {
    approve: 'bg-primary-container/40 text-primary',
    confirm: 'bg-tertiary-container/40 text-on-tertiary-container',
    deny: 'bg-error-container/40 text-on-error-container',
  }[tone]
  return (
    <div className="flex items-center gap-xs">
      <span className="text-[10px] uppercase font-mono text-outline tracking-wider">{label}</span>
      <div className="flex items-center gap-xs flex-wrap">
        {tools.map(t => (
          <code key={t} className={`text-[11px] font-mono px-xs py-[2px] rounded ${toneClass}`}>{t}</code>
        ))}
      </div>
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
      <h2 className="font-headline-md text-on-surface">New custom profile</h2>

      <div className="grid grid-cols-1 md:grid-cols-2 gap-md">
        <Field label="Name" required hint="Lowercase, hyphens or underscores only">
          <input
            value={form.name}
            onChange={e => setForm({ ...form, name: e.target.value })}
            placeholder="trusted-sandbox"
            className="w-full px-md py-sm bg-surface border border-outline-variant/50 rounded-lg focus:ring-2 focus:ring-primary outline-none font-body-sm font-mono"
          />
        </Field>

        <Field label="Description">
          <input
            value={form.description}
            onChange={e => setForm({ ...form, description: e.target.value })}
            placeholder="Full access for trusted projects"
            className="w-full px-md py-sm bg-surface border border-outline-variant/50 rounded-lg focus:ring-2 focus:ring-primary outline-none font-body-sm"
          />
        </Field>

        <Field label="Auto-approve (comma-separated)" hint={`Suggestions: ${TOOL_SUGGESTIONS.join(', ')}`}>
          <input
            value={form.auto_approve}
            onChange={e => setForm({ ...form, auto_approve: e.target.value })}
            placeholder="Read, Glob, Grep"
            className="w-full px-md py-sm bg-surface border border-outline-variant/50 rounded-lg focus:ring-2 focus:ring-primary outline-none font-body-sm font-mono"
          />
        </Field>

        <Field label="Confirm (comma-separated)" hint="Tools that prompt the user">
          <input
            value={form.confirm}
            onChange={e => setForm({ ...form, confirm: e.target.value })}
            placeholder="Edit, Write"
            className="w-full px-md py-sm bg-surface border border-outline-variant/50 rounded-lg focus:ring-2 focus:ring-primary outline-none font-body-sm font-mono"
          />
        </Field>

        <Field label="Deny (comma-separated)" hint="Always-blocked tools">
          <input
            value={form.deny}
            onChange={e => setForm({ ...form, deny: e.target.value })}
            placeholder="Bash"
            className="w-full px-md py-sm bg-surface border border-outline-variant/50 rounded-lg focus:ring-2 focus:ring-primary outline-none font-body-sm font-mono"
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
          disabled={saving || !form.name.trim()}
          className="px-lg py-sm bg-primary text-on-primary rounded-lg font-label-md cursor-pointer hover:bg-primary/90 disabled:opacity-50 disabled:cursor-not-allowed flex items-center gap-sm"
        >
          {saving && <span className="material-symbols-outlined text-[16px] animate-spin">progress_activity</span>}
          {saving ? 'Saving…' : 'Create profile'}
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
