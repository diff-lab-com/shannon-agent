import { useState, useEffect } from 'react'
import { useIntl } from 'react-intl'
import { toast } from 'sonner'
import { toastError } from '@/lib/errorToast'
import { ConfirmDialog } from '@/components/ui/confirm-dialog'
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

const AUTO_APPROVE_LABELS: { key: keyof BuiltinProfileInfo; labelKey: string }[] = [
  { key: 'auto_approve_read', labelKey: 'profiles.rule.auto' },
  { key: 'auto_approve_write', labelKey: 'profiles.rule.auto' },
  { key: 'auto_approve_bash', labelKey: 'profiles.rule.auto' },
  { key: 'auto_approve_delete', labelKey: 'profiles.rule.auto' },
  { key: 'auto_approve_network', labelKey: 'profiles.rule.auto' },
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

// Detect tools that are simultaneously denied and allowed (auto-approve or
// confirm) within a single profile — a contradictory misconfiguration.
function ruleConflicts(auto: string[], confirm: string[], deny: string[]): string[] {
  const allowed = new Set([...auto, ...confirm].map(s => s.toLowerCase()))
  return [...new Set(deny.map(s => s.toLowerCase()))].filter(t => allowed.has(t))
}

export default function Profiles() {
  const intl = useIntl()
  const t = (id: string, values?: Record<string, any>) => intl.formatMessage({ id }, values)
  const [builtin, setBuiltin] = useState<BuiltinProfileInfo[]>([])
  const [custom, setCustom] = useState<CustomProfileInfo[]>([])
  const [loading, setLoading] = useState(true)
  const [showCreate, setShowCreate] = useState(false)
  const [form, setForm] = useState<FormState>(EMPTY_FORM)
  const [saving, setSaving] = useState(false)
  const [pendingDelete, setPendingDelete] = useState<string | null>(null)

  const load = async () => {
    try {
      const data = await api.listPermissionProfiles()
      setBuiltin(data.builtin)
      setCustom(data.custom)
    } catch (e) {
      toastError(t('profiles.error.load'), e)
    }
    setLoading(false)
  }

  useEffect(() => { load() }, [])

  const handleSave = async () => {
    const name = form.name.trim()
    if (!name) {
      toast.error(t('profiles.error.nameRequired'))
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
      toast.success(t('profiles.toast.created', { name }))
      setForm(EMPTY_FORM)
      setShowCreate(false)
      await load()
    } catch (e) {
      toastError(t('profiles.error.save'), e)
    }
    setSaving(false)
  }

  const handleDelete = (name: string) => setPendingDelete(name)

  const confirmDelete = async () => {
    const name = pendingDelete
    if (!name) return
    try {
      await api.deleteCustomProfile(name)
      toast.success(t('profiles.toast.deleted', { name }))
      await load()
    } catch (e) {
      toastError(t('profiles.error.delete'), e)
    } finally {
      setPendingDelete(null)
    }
  }

  return (
    <div className="p-xl space-y-lg max-w-4xl">
      <header className="flex items-start justify-between gap-md">
        <div>
          <h1 className="font-headline-lg text-on-surface mb-xs">{t('profiles.title')}</h1>
          <p className="font-body-md text-on-surface-variant">
            {t('profiles.subtitle')}
            <code className="font-mono bg-surface-container-high px-xs rounded text-[12px]">{t('profiles.subtitle.code')}</code>{' '}
            {t('profiles.subtitle.end')}
            <code className="font-mono bg-surface-container-high px-xs rounded text-[12px]">{t('profiles.subtitle.path')}</code>
            {t('profiles.subtitle.end2')}
          </p>
        </div>
        <button
          onClick={() => setShowCreate(s => !s)}
          aria-expanded={showCreate}
          className="px-lg py-sm bg-primary text-on-primary rounded-lg font-label-md cursor-pointer hover:bg-primary/90 transition-colors flex items-center gap-sm shrink-0 focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-primary"
        >
          <span className="material-symbols-outlined text-[18px]">{showCreate ? 'close' : 'add'}</span>
          {showCreate ? t('profiles.cancel') : t('profiles.newProfile')}
        </button>
      </header>

      {showCreate && (
        <CreateForm form={form} setForm={setForm} onSave={handleSave} saving={saving} onCancel={() => setShowCreate(false)} existingNames={[...custom.map(p => p.name), ...builtin.map(p => p.id)]} />
      )}

      {loading ? (
        <div className="flex items-center justify-center py-xl">
          <span className="material-symbols-outlined icon-xl text-primary animate-spin">progress_activity</span>
        </div>
      ) : (
        <>
          <section className="space-y-sm">
            <h2 className="font-headline-md text-on-surface">{t('profiles.builtin')}</h2>
            <div className="grid grid-cols-1 md:grid-cols-3 gap-sm">
              {builtin.map(p => (
                <article key={p.id} className="bg-surface-container-lowest border border-outline-variant/30 rounded-xl p-md">
                  <header className="flex items-center justify-between gap-xs mb-xs">
                    <code className="font-mono font-headline-md text-on-surface capitalize">{p.id}</code>
                  </header>
                  <p className="font-body-sm text-on-surface-variant mb-sm">{p.description}</p>
                  <div className="flex items-center gap-xs flex-wrap">
                    {AUTO_APPROVE_LABELS.map(({ key, labelKey }) => (
                      <span
                        key={key}
                        title={`${t(labelKey)}: ${p[key] ? 'auto' : 'needs approval'}`}
                        className={`text-[10px] font-mono px-xs py-[2px] rounded ${
                          p[key]
                            ? 'bg-primary-container/40 text-primary'
                            : 'bg-surface-container-high text-outline'
                        }`}
                      >
                        {t(labelKey)}
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
              <h2 className="font-headline-md text-on-surface">{t('profiles.custom')}</h2>
              <span className="font-label-sm text-on-surface-variant">{custom.length} {custom.length === 1 ? t('profiles.count', { count: 1 }) : t('profiles.count', { count: custom.length })}</span>
            </div>
            {custom.length === 0 ? (
              <div className="bg-surface-container-lowest border border-outline-variant/30 rounded-xl p-xl text-center">
                <span className="material-symbols-outlined icon-2xl text-outline-variant block mb-sm">person_off</span>
                <p className="font-headline-md text-on-surface mb-xs">{t('profiles.empty.title')}</p>
                <p className="font-body-sm text-on-surface-variant">{t('profiles.empty.description')}</p>
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
      <ConfirmDialog
        open={pendingDelete !== null}
        title={t('profiles.confirmDelete.title')}
        message={t('profiles.confirmDelete.message', { name: pendingDelete ?? '' })}
        confirmLabel={t('profiles.confirmDelete.confirm')}
        cancelLabel={t('profiles.confirmDelete.cancel')}
        destructive
        onConfirm={() => void confirmDelete()}
        onCancel={() => setPendingDelete(null)}
      />
    </div>
  )
}

function ProfileRow({ profile, onDelete }: { profile: CustomProfileInfo; onDelete: (name: string) => void }) {
  const intl = useIntl()
  const t = (id: string, values?: Record<string, any>) => intl.formatMessage({ id }, values)
  const conflicts = ruleConflicts(profile.auto_approve, profile.confirm, profile.deny)
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
          aria-label={t('profiles.delete.aria', { name: profile.name })}
          className="p-xs rounded-lg text-on-surface-variant hover:text-error hover:bg-error-container/30 cursor-pointer transition-colors focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-primary"
        >
          <span className="material-symbols-outlined icon-md">delete</span>
        </button>
      </div>
      <div className="flex flex-wrap gap-md font-label-sm">
        <RuleChip labelKey="profiles.rule.auto" tools={profile.auto_approve} tone="approve" />
        <RuleChip labelKey="profiles.rule.confirm" tools={profile.confirm} tone="confirm" />
        <RuleChip labelKey="profiles.rule.deny" tools={profile.deny} tone="deny" />
      </div>
      {conflicts.length > 0 && (
        <p className="mt-sm text-[11px] text-error flex items-center gap-xs">
          <span className="material-symbols-outlined text-[14px]">warning</span>
          {t('profiles.conflict.ruleConflict', { tools: conflicts.join(', ') })}
        </p>
      )}
    </div>
  )
}

function RuleChip({ labelKey, tools, tone }: { labelKey: string; tools: string[]; tone: 'approve' | 'confirm' | 'deny' }) {
  if (tools.length === 0) return null
  const intl = useIntl()
  const toneClass = {
    approve: 'bg-primary-container/40 text-primary',
    confirm: 'bg-tertiary-container/40 text-on-tertiary-container',
    deny: 'bg-error-container/40 text-on-error-container',
  }[tone]
  return (
    <div className="flex items-center gap-xs">
      <span className="text-[10px] uppercase font-mono text-outline tracking-wider">{intl.formatMessage({ id: labelKey })}</span>
      <div className="flex items-center gap-xs flex-wrap">
        {tools.map(t => (
          <code key={t} className={`text-[11px] font-mono px-xs py-[2px] rounded ${toneClass}`}>{t}</code>
        ))}
      </div>
    </div>
  )
}

function CreateForm({ form, setForm, onSave, onCancel, saving, existingNames }: {
  form: FormState
  setForm: (f: FormState) => void
  onSave: () => void
  onCancel: () => void
  saving: boolean
  existingNames: string[]
}) {
  const intl = useIntl()
  const t = (id: string, values?: Record<string, any>) => intl.formatMessage({ id }, values)
  const trimmedName = form.name.trim()
  const dupName = trimmedName.length > 0 && existingNames.some(n => n.toLowerCase() === trimmedName.toLowerCase())
  const conflicts = ruleConflicts(parseList(form.auto_approve), parseList(form.confirm), parseList(form.deny))
  const blocked = dupName || conflicts.length > 0
  return (
    <section className="bg-surface-container-lowest border border-outline-variant/30 rounded-xl p-lg space-y-md">
      <h2 className="font-headline-md text-on-surface">{t('profiles.new.title')}</h2>

      <div className="grid grid-cols-1 md:grid-cols-2 gap-md">
        <Field label={t('profiles.form.name')} required hint={t('profiles.form.name.hint')}>
          <input
            value={form.name}
            onChange={e => setForm({ ...form, name: e.target.value })}
            placeholder={t('profiles.form.name.placeholder')}
            aria-invalid={dupName}
            className="w-full px-md py-sm bg-surface border border-outline-variant/50 rounded-lg focus:ring-2 focus:ring-primary outline-none font-body-sm font-mono"
          />
          {dupName && (
            <span className="font-label-sm text-error flex items-center gap-xs mt-xs">
              <span className="material-symbols-outlined text-[14px]">warning</span>
              {t('profiles.conflict.duplicateName')}
            </span>
          )}
        </Field>

        <Field label={t('profiles.form.description')}>
          <input
            value={form.description}
            onChange={e => setForm({ ...form, description: e.target.value })}
            placeholder={t('profiles.form.description.placeholder')}
            className="w-full px-md py-sm bg-surface border border-outline-variant/50 rounded-lg focus:ring-2 focus:ring-primary outline-none font-body-sm"
          />
        </Field>

        <Field label={t('profiles.form.autoApprove')} hint={t('profiles.form.autoApprove.hint', { suggestions: TOOL_SUGGESTIONS.join(', ') })}>
          <input
            value={form.auto_approve}
            onChange={e => setForm({ ...form, auto_approve: e.target.value })}
            placeholder={t('profiles.form.autoApprove.placeholder')}
            className="w-full px-md py-sm bg-surface border border-outline-variant/50 rounded-lg focus:ring-2 focus:ring-primary outline-none font-body-sm font-mono"
          />
        </Field>

        <Field label={t('profiles.form.confirm')} hint={t('profiles.form.confirm.hint')}>
          <input
            value={form.confirm}
            onChange={e => setForm({ ...form, confirm: e.target.value })}
            placeholder={t('profiles.form.confirm.placeholder')}
            className="w-full px-md py-sm bg-surface border border-outline-variant/50 rounded-lg focus:ring-2 focus:ring-primary outline-none font-body-sm font-mono"
          />
        </Field>

        <Field label={t('profiles.form.deny')} hint={t('profiles.form.deny.hint')}>
          <input
            value={form.deny}
            onChange={e => setForm({ ...form, deny: e.target.value })}
            placeholder={t('profiles.form.deny.placeholder')}
            className="w-full px-md py-sm bg-surface border border-outline-variant/50 rounded-lg focus:ring-2 focus:ring-primary outline-none font-body-sm font-mono"
          />
        </Field>
      </div>

      {blocked && (
        <div className="text-error font-label-sm flex items-center gap-xs pt-xs">
          <span className="material-symbols-outlined text-[16px]">warning</span>
          {conflicts.length > 0 ? t('profiles.conflict.ruleConflict', { tools: conflicts.join(', ') }) : t('profiles.conflict.duplicateName')}
        </div>
      )}
      <div className="flex justify-end gap-sm pt-sm">
        <button
          onClick={onCancel}
          disabled={saving}
          className="px-md py-sm text-on-surface-variant hover:text-primary font-label-md cursor-pointer disabled:opacity-50"
        >
          {t('profiles.form.cancel')}
        </button>
        <button
          onClick={onSave}
          disabled={saving || !form.name.trim() || blocked}
          className="px-lg py-sm bg-primary text-on-primary rounded-lg font-label-md cursor-pointer hover:bg-primary/90 disabled:opacity-50 disabled:cursor-not-allowed flex items-center gap-sm"
        >
          {saving && <span className="material-symbols-outlined icon-sm animate-spin">progress_activity</span>}
          {saving ? t('profiles.form.saving') : t('profiles.form.create')}
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
