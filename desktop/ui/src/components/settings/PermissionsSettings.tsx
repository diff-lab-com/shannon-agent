import { useEffect, useMemo, useState } from 'react'
import { useIntl, type PrimitiveType } from 'react-intl'
import { toast } from 'sonner'
import { Button } from '@/components/ui/button'
import { Modal } from '@/components/ui/modal'
import { ConfirmDialog } from '@/components/ui/confirm-dialog'
import { Input } from '@/components/ui/input'
import { Spinner } from '@/components/ui/loading-state'
import { cn } from '@/lib/utils'
import * as api from '@/lib/tauri-api'
import type { BuiltinProfileInfo, CustomProfileInfo, ProfilesList } from '@/types'
import { useCatalog } from '@/context/CatalogContext'
import { toastError } from '@/lib/errorToast'

// ─── Rule-list input validation (P1-3) ──────────────────────────────────────

/**
 * Validate one permission-rule entry against the rule syntax the engine's
 * `PermissionRuleChecker` understands:
 *
 * - a bare tool name (`Bash`, `Read`) — case-insensitive match on the tool;
 * - `Tool(command_pattern)` — e.g. `Bash(git push *)`, `Bash(git *)`;
 * - a plain glob (`mcp__github__*`) compiled by the engine's globset.
 *
 * Returns an i18n error key when invalid, `null` when acceptable.
 */
export function validateRuleInput(raw: string): string | null {
  const rule = raw.trim()
  if (rule === '') return 'settings.permissions.rules.error.empty'
  if (rule.length > 200) return 'settings.permissions.rules.error.tooLong'
  const paren = rule.indexOf('(')
  if (paren >= 0) {
    // Structured form: `Tool(pattern)` — the tool part must be non-empty and
    // space-free; the pattern inside may contain spaces (e.g. `git push *`).
    if (!rule.endsWith(')')) return 'settings.permissions.rules.error.unclosed'
    const tool = rule.slice(0, paren).trim()
    if (tool === '') return 'settings.permissions.rules.error.emptyTool'
    if (/\s/.test(tool)) return 'settings.permissions.rules.error.toolWhitespace'
    return null
  }
  // Bare tool / glob form — no whitespace allowed.
  if (/\s/.test(rule)) return 'settings.permissions.rules.error.whitespace'
  return null
}

// ─── Page ───────────────────────────────────────────────────────────────────

type RuleGroup = 'auto_approve' | 'confirm' | 'deny'

interface EditorState {
  /** Original name when editing (rename = save under the new name). */
  originalName: string | null
  name: string
  description: string
  auto_approve: string[]
  confirm: string[]
  deny: string[]
}

const EMPTY_EDITOR: EditorState = {
  originalName: null,
  name: '',
  description: '',
  auto_approve: [],
  confirm: [],
  deny: [],
}

/** P1-3 Settings → 权限配置 (permission profiles + command sandbox). */
export default function PermissionsSettings() {
  const intl = useIntl()
  const t = (id: string, values?: Record<string, PrimitiveType>) =>
    intl.formatMessage({ id }, values)
  const { config, refreshConfig } = useCatalog()

  const [profiles, setProfiles] = useState<ProfilesList | null>(null)
  const [loading, setLoading] = useState(true)
  const [activating, setActivating] = useState<string | null>(null)
  const [editor, setEditor] = useState<EditorState | null>(null)
  const [deleteTarget, setDeleteTarget] = useState<string | null>(null)
  const [deleting, setDeleting] = useState(false)
  const [sandboxBusy, setSandboxBusy] = useState(false)

  const activeProfile = config?.active_permission_profile ?? null

  const loadProfiles = (active = true) => {
    api
      .listPermissionProfiles()
      .then((list) => {
        if (active) setProfiles(list)
      })
      .catch(() => {
        if (active) setProfiles({ builtin: [], custom: [] })
      })
      .finally(() => {
        if (active) setLoading(false)
      })
  }

  useEffect(() => {
    let cancelled = false
    loadProfiles(!cancelled)
    return () => {
      cancelled = true
    }
  }, [])

  const sandboxMode = useMemo<'off' | 'local' | 'landlock'>(
    () => config?.sandbox?.mode ?? 'off',
    [config?.sandbox?.mode],
  )

  const handleActivate = async (name: string | null) => {
    setActivating(name ?? '__clear__')
    try {
      await api.activatePermissionProfile(name)
      await refreshConfig()
      toast.success(
        name == null
          ? t('settings.permissions.toast.cleared')
          : t('settings.permissions.toast.activated', { name }),
      )
    } catch (e) {
      toastError(t('settings.permissions.toast.failed'), e)
    } finally {
      setActivating(null)
    }
  }

  const handleDelete = async () => {
    if (deleteTarget == null) return
    setDeleting(true)
    try {
      const removed = await api.deleteCustomProfile(deleteTarget)
      // Deactivate if the deleted profile was active.
      if (activeProfile === deleteTarget) {
        await api.activatePermissionProfile(null)
        await refreshConfig()
      }
      if (removed.length === 0) {
        toast.warning(t('settings.permissions.toast.deleteMissing'))
      } else {
        toast.success(t('settings.permissions.toast.deleted', { name: deleteTarget }))
      }
      setDeleteTarget(null)
      loadProfiles()
    } catch (e) {
      toastError(t('settings.permissions.toast.deleteFailed'), e)
    } finally {
      setDeleting(false)
    }
  }

  const handleSaveProfile = async () => {
    if (!editor) return
    try {
      await api.saveCustomProfile({
        name: editor.name,
        description: editor.description || undefined,
        auto_approve: editor.auto_approve,
        confirm: editor.confirm,
        deny: editor.deny,
      })
      toast.success(t('settings.permissions.toast.saved', { name: editor.name }))
      setEditor(null)
      loadProfiles()
    } catch (e) {
      toastError(t('settings.permissions.toast.saveFailed'), e)
    }
  }

  const handleSandboxMode = async (mode: 'off' | 'local' | 'landlock') => {
    if (mode === sandboxMode || sandboxBusy) return
    setSandboxBusy(true)
    try {
      await api.configure({ key: 'sandbox.mode', value: mode })
      await refreshConfig()
      toast.success(t('settings.permissions.sandbox.toastSet'))
    } catch (e) {
      toastError(t('settings.permissions.sandbox.toastFailed'), e)
    } finally {
      setSandboxBusy(false)
    }
  }

  const editorValid =
    editor != null &&
    editor.name.trim() !== '' &&
    /^[A-Za-z0-9_-]+$/.test(editor.name.trim()) &&
    [...editor.auto_approve, ...editor.confirm, ...editor.deny].every(
      (r) => validateRuleInput(r) == null,
    )

  return (
    <div className="space-y-xl" data-testid="permissions-settings">
      <div>
        <h2 className="font-headline-md text-on-surface font-bold">{t('settings.permissions.title')}</h2>
        <p className="text-body-sm text-on-surface-variant mt-xs">{t('settings.permissions.intro')}</p>
      </div>

      {loading ? (
        <div className="py-xl flex justify-center" role="status">
          <Spinner />
          <span className="sr-only">{t('settings.permissions.loading')}</span>
        </div>
      ) : (
        <>
          {/* Built-in tiers */}
          <section aria-label={t('settings.permissions.builtin.aria')} className="space-y-sm">
            <h3 className="font-title-md text-on-surface font-semibold">{t('settings.permissions.builtin.title')}</h3>
            <p className="text-body-sm text-on-surface-variant">{t('settings.permissions.builtin.diff')}</p>
            <div className="grid gap-md md:grid-cols-3">
              {(profiles?.builtin ?? []).map((p: BuiltinProfileInfo) => (
                <BuiltinCard
                  key={p.id}
                  profile={p}
                  active={activeProfile === p.id}
                  busy={activating != null}
                  onActivate={() => void handleActivate(p.id)}
                />
              ))}
            </div>
          </section>

          {/* Custom profiles */}
          <section aria-label={t('settings.permissions.custom.aria')} className="space-y-sm">
            <div className="flex items-center justify-between">
              <h3 className="font-title-md text-on-surface font-semibold">{t('settings.permissions.custom.title')}</h3>
              <Button
                className="flex items-center gap-xs px-md py-sm rounded-lg bg-primary text-on-primary font-label-md"
                onClick={() => setEditor({ ...EMPTY_EDITOR })}
              >
                <span className="material-symbols-outlined icon-md" aria-hidden="true">add</span>
                {t('settings.permissions.custom.create')}
              </Button>
            </div>
            {profiles && profiles.custom.length === 0 ? (
              <p className="text-body-sm text-on-surface-variant">{t('settings.permissions.custom.empty')}</p>
            ) : (
              <ul className="space-y-sm">
                {(profiles?.custom ?? []).map((p: CustomProfileInfo) => (
                  <li
                    key={p.name}
                    className={cn(
                      'flex items-center gap-md p-md rounded-xl border bg-surface-container-low',
                      activeProfile === p.name ? 'border-primary/50' : 'border-outline-variant/20',
                    )}
                  >
                    <div className="flex-1 min-w-0">
                      <div className="flex items-center gap-sm">
                        <span className="font-title-sm text-on-surface font-semibold truncate">{p.name}</span>
                        {activeProfile === p.name && (
                          <span className="px-sm py-xs rounded-full bg-primary/10 text-primary font-label-sm text-[11px] font-bold uppercase tracking-wider">
                            {t('settings.permissions.activeBadge')}
                          </span>
                        )}
                      </div>
                      {p.description && (
                        <p className="text-body-sm text-on-surface-variant truncate">{p.description}</p>
                      )}
                      <p className="font-mono text-label-sm text-on-surface-variant truncate mt-xs">
                        {t('settings.permissions.custom.rulesSummary', {
                          auto: p.auto_approve.length,
                          confirm: p.confirm.length,
                          deny: p.deny.length,
                        })}
                      </p>
                    </div>
                    <div className="flex items-center gap-sm shrink-0">
                      <Button
                        variant="ghost"
                        aria-label={t('settings.permissions.custom.edit')}
                        title={t('settings.permissions.custom.edit')}
                        className="p-sm rounded-lg text-on-surface-variant hover:text-primary"
                        onClick={() =>
                          setEditor({
                            originalName: p.name,
                            name: p.name,
                            description: p.description ?? '',
                            auto_approve: [...p.auto_approve],
                            confirm: [...p.confirm],
                            deny: [...p.deny],
                          })
                        }
                      >
                        <span className="material-symbols-outlined icon-md" aria-hidden="true">edit</span>
                      </Button>
                      <Button
                        variant="ghost"
                        aria-label={t('settings.permissions.custom.delete')}
                        title={t('settings.permissions.custom.delete')}
                        className="p-sm rounded-lg text-on-surface-variant hover:text-error"
                        onClick={() => setDeleteTarget(p.name)}
                      >
                        <span className="material-symbols-outlined icon-md" aria-hidden="true">delete</span>
                      </Button>
                      <Button
                        className={cn(
                          'px-md py-sm rounded-lg font-label-md',
                          activeProfile === p.name
                            ? 'bg-primary/10 text-primary'
                            : 'bg-primary text-on-primary',
                        )}
                        disabled={activeProfile === p.name || activating != null}
                        onClick={() => void handleActivate(p.name)}
                      >
                        {activeProfile === p.name
                          ? t('settings.permissions.isActive')
                          : t('settings.permissions.activate')}
                      </Button>
                    </div>
                  </li>
                ))}
              </ul>
            )}
          </section>

          {/* Command sandbox (P1-3) */}
          <section aria-label={t('settings.permissions.sandbox.aria')} className="space-y-sm p-md rounded-xl border border-outline-variant/20 bg-surface-container-low">
            <h3 className="font-title-md text-on-surface font-semibold">{t('settings.permissions.sandbox.title')}</h3>
            <p className="text-body-sm text-on-surface-variant">{t('settings.permissions.sandbox.desc')}</p>
            <div className="flex flex-wrap gap-sm" role="radiogroup" aria-label={t('settings.permissions.sandbox.aria')}>
              {(['off', 'local', 'landlock'] as const).map((mode) => (
                <Button
                  key={mode}
                  role="radio"
                  aria-checked={sandboxMode === mode}
                  className={cn(
                    'px-md py-sm rounded-lg font-label-md border',
                    sandboxMode === mode
                      ? 'bg-primary text-on-primary border-primary'
                      : 'bg-transparent text-on-surface border-outline-variant/30 hover:bg-surface-container',
                  )}
                  disabled={sandboxBusy}
                  onClick={() => void handleSandboxMode(mode)}
                >
                  {t(`settings.permissions.sandbox.${mode}`)}
                </Button>
              ))}
            </div>
            <p className="text-label-md text-on-surface-variant flex items-center gap-xs">
              <span className="material-symbols-outlined text-[14px]" aria-hidden="true">science</span>
              {t('settings.permissions.sandbox.landlockNote')}
            </p>
            <p className="text-label-md text-warning flex items-center gap-xs">
              <span className="material-symbols-outlined text-[14px]" aria-hidden="true">restart_alt</span>
              {t('settings.permissions.sandbox.restartNote')}
            </p>
          </section>

          {/* Rule syntax guide */}
          <section aria-label={t('settings.permissions.rules.title')} className="space-y-xs">
            <h3 className="font-title-md text-on-surface font-semibold">{t('settings.permissions.rules.title')}</h3>
            <p className="text-body-sm text-on-surface-variant">{t('settings.permissions.rules.intro')}</p>
            <ul className="text-body-sm text-on-surface-variant list-disc pl-lg space-y-xs">
              <li><code className="font-mono">Bash</code> — {t('settings.permissions.rules.bare')}</li>
              <li><code className="font-mono">Bash(git push *)</code> — {t('settings.permissions.rules.structured')}</li>
              <li><code className="font-mono">mcp__github__*</code> — {t('settings.permissions.rules.glob')}</li>
            </ul>
          </section>
        </>
      )}

      {/* Create / edit modal */}
      {editor && (
        <ProfileEditorModal
          editor={editor}
          onChange={setEditor}
          onCancel={() => setEditor(null)}
          onSave={() => void handleSaveProfile()}
          valid={editorValid}
        />
      )}

      <ConfirmDialog
        open={deleteTarget != null}
        title={t('settings.permissions.delete.title')}
        message={t('settings.permissions.delete.message', { name: deleteTarget ?? '' })}
        confirmLabel={t('settings.permissions.delete.confirm')}
        cancelLabel={t('settings.permissions.delete.cancel')}
        destructive
        busy={deleting}
        busyLabel={t('settings.permissions.delete.busy')}
        onConfirm={() => void handleDelete()}
        onCancel={() => setDeleteTarget(null)}
      />
    </div>
  )
}

function BuiltinCard({
  profile,
  active,
  busy,
  onActivate,
}: {
  profile: BuiltinProfileInfo
  active: boolean
  busy: boolean
  onActivate: () => void
}) {
  const intl = useIntl()
  const t = (id: string, values?: Record<string, PrimitiveType>) =>
    intl.formatMessage({ id }, values)
  const flags: Array<[boolean, string]> = [
    [profile.auto_approve_read, 'settings.permissions.builtin.flag.read'],
    [profile.auto_approve_write, 'settings.permissions.builtin.flag.write'],
    [profile.auto_approve_bash, 'settings.permissions.builtin.flag.bash'],
    [profile.auto_approve_delete, 'settings.permissions.builtin.flag.delete'],
  ]
  return (
    <div
      className={cn(
        'flex flex-col gap-sm p-md rounded-xl border bg-surface-container-low',
        active ? 'border-primary/50' : 'border-outline-variant/20',
      )}
    >
      <div className="flex items-center gap-sm">
        <span className="material-symbols-outlined icon-md text-primary" aria-hidden="true">
          {profile.id === 'strict' ? 'shield_lock' : profile.id === 'permissive' ? 'speed' : 'balance'}
        </span>
        <span className="font-title-sm text-on-surface font-semibold capitalize">{profile.id}</span>
        {active && (
          <span className="ml-auto px-sm py-xs rounded-full bg-primary/10 text-primary font-label-sm text-[11px] font-bold uppercase tracking-wider">
            {t('settings.permissions.activeBadge')}
          </span>
        )}
      </div>
      <p className="text-body-sm text-on-surface-variant">{profile.description}</p>
      <ul className="text-label-md text-on-surface-variant space-y-xs">
        {flags.map(([on, key]) => (
          <li key={key} className="flex items-center gap-xs">
            <span className={cn('material-symbols-outlined text-[14px]', on ? 'text-primary' : 'text-on-surface-variant/60')} aria-hidden="true">
              {on ? 'check_circle' : 'radio_button_unchecked'}
            </span>
            {t(key)}
          </li>
        ))}
      </ul>
      {profile.deny_destructive.length > 0 && (
        <p className="text-label-md text-error/90">
          {t('settings.permissions.builtin.denyList', { tools: profile.deny_destructive.join(', ') })}
        </p>
      )}
      <Button
        className={cn(
          'mt-auto px-md py-sm rounded-lg font-label-md',
          active ? 'bg-primary/10 text-primary' : 'bg-primary text-on-primary',
        )}
        disabled={active || busy}
        onClick={onActivate}
      >
        {active ? t('settings.permissions.isActive') : t('settings.permissions.activate')}
      </Button>
    </div>
  )
}

function ProfileEditorModal({
  editor,
  onChange,
  onCancel,
  onSave,
  valid,
}: {
  editor: EditorState
  onChange: (next: EditorState) => void
  onCancel: () => void
  onSave: () => void
  valid: boolean
}) {
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })
  const groups: Array<{ key: RuleGroup; titleKey: string; hintKey: string }> = [
    { key: 'auto_approve', titleKey: 'settings.permissions.editor.auto.title', hintKey: 'settings.permissions.editor.auto.hint' },
    { key: 'confirm', titleKey: 'settings.permissions.editor.confirm.title', hintKey: 'settings.permissions.editor.confirm.hint' },
    { key: 'deny', titleKey: 'settings.permissions.editor.deny.title', hintKey: 'settings.permissions.editor.deny.hint' },
  ]

  const addRule = (group: RuleGroup, value: string) => {
    if (value.trim() === '') return
    onChange({ ...editor, [group]: [...editor[group], value.trim()] })
  }
  const removeRule = (group: RuleGroup, index: number) => {
    onChange({ ...editor, [group]: editor[group].filter((_, i) => i !== index) })
  }

  return (
    <Modal
      open
      onClose={onCancel}
      title={
        editor.originalName
          ? t('settings.permissions.editor.titleEdit')
          : t('settings.permissions.editor.titleCreate')
      }
      size="lg"
    >
      <div className="space-y-md max-h-[70vh] overflow-y-auto pr-sm">
        <div className="grid gap-md md:grid-cols-2">
          <label className="space-y-xs">
            <span className="text-label-md text-on-surface-variant">{t('settings.permissions.editor.name')}</span>
            <Input
              value={editor.name}
              onChange={(e) => onChange({ ...editor, name: e.target.value })}
              placeholder="my-profile"
              aria-label={t('settings.permissions.editor.name')}
            />
            {editor.name.trim() !== '' && !/^[A-Za-z0-9_-]+$/.test(editor.name.trim()) && (
              <span className="text-label-sm text-error">{t('settings.permissions.editor.nameInvalid')}</span>
            )}
          </label>
          <label className="space-y-xs">
            <span className="text-label-md text-on-surface-variant">{t('settings.permissions.editor.description')}</span>
            <Input
              value={editor.description}
              onChange={(e) => onChange({ ...editor, description: e.target.value })}
              aria-label={t('settings.permissions.editor.description')}
            />
          </label>
        </div>

        {groups.map((group) => (
          <RuleGroupEditor
            key={group.key}
            title={t(group.titleKey)}
            hint={t(group.hintKey)}
            rules={editor[group.key]}
            onAdd={(value) => addRule(group.key, value)}
            onRemove={(index) => removeRule(group.key, index)}
          />
        ))}
      </div>
      <div className="flex justify-end gap-md pt-md">
        <Button variant="ghost" className="px-md py-sm rounded-lg font-label-md" onClick={onCancel}>
          {t('settings.permissions.editor.cancel')}
        </Button>
        <Button
          className="px-md py-sm rounded-lg bg-primary text-on-primary font-label-md"
          disabled={!valid}
          onClick={onSave}
        >
          {t('settings.permissions.editor.save')}
        </Button>
      </div>
    </Modal>
  )
}

function RuleGroupEditor({
  title,
  hint,
  rules,
  onAdd,
  onRemove,
}: {
  title: string
  hint: string
  rules: string[]
  onAdd: (value: string) => void
  onRemove: (index: number) => void
}) {
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })
  const [draft, setDraft] = useState('')
  const errorKey = draft === '' ? null : validateRuleInput(draft)

  return (
    <fieldset className="space-y-xs p-md rounded-xl border border-outline-variant/20">
      <legend className="px-xs font-label-md text-on-surface font-semibold">{title}</legend>
      <p className="text-label-md text-on-surface-variant">{hint}</p>
      {rules.length > 0 && (
        <ul className="space-y-xs">
          {rules.map((rule, index) => (
            <li key={`${rule}-${index}`} className="flex items-center gap-sm">
              <code className="font-mono text-body-sm text-on-surface bg-surface-container px-sm py-xs rounded flex-1 truncate">
                {rule}
              </code>
              <Button
                variant="ghost"
                aria-label={t('settings.permissions.editor.removeRule')}
                className="p-xs rounded text-on-surface-variant hover:text-error"
                onClick={() => onRemove(index)}
              >
                <span className="material-symbols-outlined icon-sm" aria-hidden="true">close</span>
              </Button>
            </li>
          ))}
        </ul>
      )}
      <div className="flex gap-sm">
        <Input
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          placeholder="Bash(git push *)"
          aria-label={title}
          onKeyDown={(e) => {
            if (e.key === 'Enter' && errorKey == null && draft.trim() !== '') {
              onAdd(draft)
              setDraft('')
            }
          }}
        />
        <Button
          variant="ghost"
          className="px-md py-sm rounded-lg text-primary font-label-md"
          disabled={draft.trim() === '' || errorKey != null}
          onClick={() => {
            onAdd(draft)
            setDraft('')
          }}
        >
          {t('settings.permissions.editor.addRule')}
        </Button>
      </div>
      {errorKey && <p className="text-label-sm text-error">{t(errorKey)}</p>}
    </fieldset>
  )
}
