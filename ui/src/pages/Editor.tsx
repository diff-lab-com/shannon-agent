// Editor page — load a source file, render it with CodeMirror, auto-fetch
// LSP diagnostics, and let the user add manual diagnostic squiggles too.
// Clicking a squiggle opens the LspQuickFixPanel in a side drawer.
//
// Phase E1 v2: auto-LSP diagnostics via publishDiagnostics subscription.
// Phase E1 v1: manual squiggle UI.

import { useEffect, useState, useCallback, useRef } from 'react'
import { useNavigate } from 'react-router-dom'
import { useIntl } from 'react-intl'
import { open as openDialog } from '@tauri-apps/plugin-dialog'
import { toast } from 'sonner'
import CodeEditor, {
  type EditorDiagnostic,
} from '@/components/editor/CodeEditor'
import LspQuickFixPanel from '@/components/lsp/LspQuickFixPanel'
import { useModalFocus } from '@/hooks/useModalFocus'
import * as api from '@/lib/tauri-api'
import type { SourceFile } from '@/lib/tauri-api'

interface AutoDiagnostic extends EditorDiagnostic {
  kind: 'auto'
  source?: string
  code?: string
}

interface ManualDiagnostic extends EditorDiagnostic {
  kind: 'manual'
}

type MixedDiagnostic = AutoDiagnostic | ManualDiagnostic

interface DrawerDiag {
  file_path: string
  start_line: number
  start_character: number
  end_line: number
  end_character: number
  message: string
  language_id: string
}

const SEVERITIES: EditorDiagnostic['severity'][] = [
  'error',
  'warning',
  'info',
  'hint',
]

function normalizeSeverity(raw: string): EditorDiagnostic['severity'] {
  const lower = raw.toLowerCase()
  if (lower === 'error') return 'error'
  if (lower === 'warning') return 'warning'
  if (lower === 'info' || lower === 'information') return 'info'
  if (lower === 'hint') return 'hint'
  return 'warning'
}

export default function Editor() {
  const intl = useIntl()
  const t = (id: string, values?: any) => intl.formatMessage({ id }, values)
  const navigate = useNavigate()
  const [filePath, setFilePath] = useState('')
  const [loading, setLoading] = useState(false)
  const [loadError, setLoadError] = useState<string | null>(null)
  const [file, setFile] = useState<SourceFile | null>(null)
  const [autoDiags, setAutoDiags] = useState<AutoDiagnostic[]>([])
  const [manualDiags, setManualDiags] = useState<ManualDiagnostic[]>([])
  const [diagLoading, setDiagLoading] = useState(false)
  const [diagError, setDiagError] = useState<string | null>(null)
  const [diagTimedOut, setDiagTimedOut] = useState(false)

  // Add-squiggle form
  const [newLine, setNewLine] = useState(0)
  const [newStartChar, setNewStartChar] = useState(0)
  const [newEndChar, setNewEndChar] = useState(1)
  const [newMessage, setNewMessage] = useState('')
  const [newSeverity, setNewSeverity] =
    useState<EditorDiagnostic['severity']>('warning')

  // Edit mode
  const [editMode, setEditMode] = useState(false)
  const [draft, setDraft] = useState('')
  const [saving, setSaving] = useState(false)

  // Side drawer for quick-fix
  const [drawer, setDrawer] = useState<DrawerDiag | null>(null)

  const drawerRef = useRef<HTMLDivElement>(null)
  useModalFocus(!!drawer, drawerRef)

  const fetchDiagnostics = useCallback(async (sourceFile: SourceFile) => {
    const server = api.defaultDiagnosticsServer(sourceFile.language_id)
    if (!server.cmd) {
      setAutoDiags([])
      setDiagError(null)
      setDiagTimedOut(false)
      return
    }
    setDiagLoading(true)
    setDiagError(null)
    setDiagTimedOut(false)
    try {
      const resp = await api.runFileDiagnostics({
        file_path: sourceFile.path,
        server_cmd: server.cmd,
        server_args: server.args,
        language_id: sourceFile.language_id,
        content: sourceFile.content,
      })
      setAutoDiags(
        resp.diagnostics.map<AutoDiagnostic>((d) => ({
          kind: 'auto',
          start_line: d.start_line,
          start_character: d.start_character,
          end_line: d.end_line,
          end_character: d.end_character,
          message: d.message,
          severity: normalizeSeverity(d.severity),
          source: d.source,
          code: d.code,
        })),
      )
      setDiagTimedOut(resp.timed_out)
    } catch (err) {
      setAutoDiags([])
      setDiagError(String(err))
    } finally {
      setDiagLoading(false)
    }
  }, [])

  const onLoad = useCallback(
    async (e: React.FormEvent) => {
      e.preventDefault()
      if (!filePath.trim()) return
      setLoading(true)
      setLoadError(null)
      try {
        const dto = await api.readSourceFile(filePath.trim())
        setFile(dto)
        setDraft(dto.content)
        setEditMode(false)
        setManualDiags([])
        void fetchDiagnostics(dto)
      } catch (err) {
        setFile(null)
        setAutoDiags([])
        setManualDiags([])
        setLoadError(String(err))
      } finally {
        setLoading(false)
      }
    },
    [filePath, fetchDiagnostics],
  )

  const onBrowse = useCallback(async () => {
    try {
      const picked = await openDialog({
        multiple: false,
        directory: false,
      })
      if (typeof picked === 'string' && picked.length > 0) {
        setFilePath(picked)
      }
    } catch (err) {
      setLoadError(String(err))
    }
  }, [])

  const onToggleEdit = useCallback(() => {
    if (!file) return
    setDraft(file.content)
    setEditMode(v => !v)
  }, [file])

  const onSave = useCallback(async () => {
    if (!file) return
    setSaving(true)
    try {
      await api.saveTextFile(file.path, draft)
      const refreshed = { ...file, content: draft }
      setFile(refreshed)
      setEditMode(false)
      void fetchDiagnostics(refreshed)
      toast.success(t('editor.saveSuccess'))
    } catch (err) {
      toast.error(t('editor.saveFailed'), { description: String(err) })
    } finally {
      setSaving(false)
    }
  }, [file, draft, fetchDiagnostics, t])

  const onAskAi = useCallback(
    (d: MixedDiagnostic) => {
      if (!file) return
      const severity = d.severity.toUpperCase()
      const loc = `${d.start_line + 1}:${d.start_character + 1}`
      const sourceTag = d.kind === 'auto' && d.source ? ` [${d.source}]` : ''
      const msg = `${file.path}:${loc} — ${severity}${sourceTag}\n${d.message}`
      navigate('/chat', { state: { prefill: msg } })
    },
    [file, navigate],
  )

  const onAddSquiggle = (e: React.FormEvent) => {
    e.preventDefault()
    if (!file) return
    if (!newMessage.trim()) return
    if (newEndChar <= newStartChar) return
    const lineMax = file.content.split('\n').length - 1
    const line = Math.min(Math.max(newLine, 0), lineMax)
    setManualDiags((d) => [
      ...d,
      {
        kind: 'manual',
        start_line: line,
        start_character: newStartChar,
        end_line: line,
        end_character: newEndChar,
        message: newMessage,
        severity: newSeverity,
      },
    ])
    setNewMessage('')
  }

  const onSquiggleClick = (diag: EditorDiagnostic) => {
    if (!file) return
    setDrawer({
      file_path: file.path,
      start_line: diag.start_line,
      start_character: diag.start_character,
      end_line: diag.end_line,
      end_character: diag.end_character,
      message: diag.message,
      language_id: file.language_id,
    })
  }

  // Close drawer on Escape
  useEffect(() => {
    if (!drawer) return
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') setDrawer(null)
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [drawer])

  const diags: MixedDiagnostic[] = [...autoDiags, ...manualDiags]
  const diagCount = diags.length

  return (
    <div className="max-w-6xl mx-auto p-md flex flex-col gap-md">
      <header>
        <h2 className="text-headline-md font-headline-md text-on-surface">{t('editor.title')}</h2>
        <p className="font-label-sm text-on-surface-variant mt-xs">
          {t('editor.subtitle')}
        </p>
      </header>

      <form
        onSubmit={onLoad}
        className="bg-surface-container-lowest rounded-2xl p-md border border-outline-variant/30 shadow-sm flex flex-col gap-sm"
      >
        <label className="font-label-sm text-on-surface-variant flex flex-col gap-xs">
          {t('editor.filePath')}
          <div className="flex gap-xs">
            <input
              type="text"
              value={filePath}
              onChange={(e) => setFilePath(e.target.value)}
              placeholder={t('editor.filePath.placeholder')}
              className="flex-1 font-mono font-label-md bg-surface-container-low text-on-surface border border-outline-variant/40 rounded-lg px-sm py-xs focus:outline-none focus-visible:ring-2 focus-visible:ring-primary/30"
            />
            <button
              type="button"
              onClick={onBrowse}
              aria-label={t('editor.browse')}
              className="flex items-center gap-xs px-md py-xs rounded-lg border border-outline-variant/40 bg-surface-container-low text-on-surface hover:bg-surface-container-high focus:outline-none focus-visible:ring-2 focus-visible:ring-primary/30 cursor-pointer"
            >
              <span className="material-symbols-outlined text-[18px]">
                folder_open
              </span>
              <span className="font-label-md">{t('editor.browse')}</span>
            </button>
          </div>
        </label>
        <button
          type="submit"
          disabled={!filePath.trim() || loading}
          className="self-start font-label-md bg-primary text-on-primary rounded-lg px-md py-sm cursor-pointer disabled:opacity-40 disabled:cursor-not-allowed hover:bg-primary/90 focus:outline-none focus-visible:ring-2 focus-visible:ring-primary/30"
        >
          {loading ? t('editor.loading') : t('editor.loadFile')}
        </button>
        {loadError ? (
          <div
            className="bg-error/10 border border-error/30 rounded-lg p-sm font-label-sm text-error"
            role="alert"
          >
            {loadError}
          </div>
        ) : null}
      </form>

      {file ? (
        <>
          <div className="flex items-center gap-sm font-label-sm text-on-surface-variant flex-wrap">
            <code className="font-mono bg-surface-container-low px-1.5 py-0.5 rounded">
              {file.path.split('/').pop()}
            </code>
            <span className="text-[11px] uppercase tracking-wider">
              {file.language_id}
            </span>
            <span>·</span>
            <span>{diagCount} {t(`editor.diagnostics`, { count: diagCount })}</span>
            <button
              type="button"
              onClick={() => void fetchDiagnostics(file)}
              disabled={diagLoading}
              className="flex items-center gap-xs px-sm py-0.5 rounded-full border border-outline-variant/40 bg-surface-container-low text-on-surface hover:bg-surface-container-high focus:outline-none focus-visible:ring-2 focus-visible:ring-primary/30 disabled:opacity-40 disabled:cursor-not-allowed cursor-pointer"
              aria-label={t('editor.reRun')}
            >
              <span
                className={
                  diagLoading
                    ? 'material-symbols-outlined text-[14px] animate-spin'
                    : 'material-symbols-outlined text-[14px]'
                }
              >
                {diagLoading ? 'progress_activity' : 'refresh'}
              </span>
              <span>{diagLoading ? t('editor.running') : t('editor.reRun')}</span>
            </button>
            <div className="ml-auto flex items-center gap-xs">
              {editMode ? (
                <>
                  <button
                    type="button"
                    onClick={onSave}
                    disabled={saving}
                    className="flex items-center gap-xs px-sm py-0.5 rounded-full bg-primary text-on-primary hover:bg-primary/90 focus:outline-none focus-visible:ring-2 focus-visible:ring-primary/30 disabled:opacity-40 disabled:cursor-not-allowed cursor-pointer"
                  >
                    <span className="material-symbols-outlined text-[14px]">
                      {saving ? 'progress_activity' : 'save'}
                    </span>
                    <span>{saving ? t('editor.saving') : t('editor.save')}</span>
                  </button>
                  <button
                    type="button"
                    onClick={onToggleEdit}
                    disabled={saving}
                    className="flex items-center gap-xs px-sm py-0.5 rounded-full border border-outline-variant/40 bg-surface-container-low text-on-surface hover:bg-surface-container-high focus:outline-none focus-visible:ring-2 focus-visible:ring-primary/30 disabled:opacity-40 disabled:cursor-not-allowed cursor-pointer"
                  >
                    <span className="material-symbols-outlined text-[14px]">
                      close
                    </span>
                    <span>{t('editor.cancel')}</span>
                  </button>
                </>
              ) : (
                <button
                  type="button"
                  onClick={onToggleEdit}
                  className="flex items-center gap-xs px-sm py-0.5 rounded-full border border-outline-variant/40 bg-surface-container-low text-on-surface hover:bg-surface-container-high focus:outline-none focus-visible:ring-2 focus-visible:ring-primary/30 cursor-pointer"
                  aria-label={t('editor.editMode')}
                >
                  <span className="material-symbols-outlined text-[14px]">edit</span>
                  <span>{t('editor.editMode')}</span>
                </button>
              )}
            </div>
          </div>

          {(diagError || diagTimedOut) && file ? (
            <div
              className="bg-error/10 border border-error/30 rounded-lg p-sm font-label-sm text-error flex items-start gap-sm"
              role="status"
            >
              <span className="material-symbols-outlined text-[16px] mt-0.5">
                warning
              </span>
              <span className="flex-1">
                {diagError
                  ? `${t('editor.diagnosticsFailed', { error: diagError })}`
                  : diagTimedOut
                    ? t('editor.diagnosticsTimedOut')
                    : null}
              </span>
            </div>
          ) : null}

          <CodeEditor
            value={editMode ? draft : file.content}
            onValueChange={editMode ? setDraft : undefined}
            language={file.language_id}
            diagnostics={diags}
            onDiagnosticClick={onSquiggleClick}
            readOnly={!editMode}
          />

          <form
            onSubmit={onAddSquiggle}
            className="bg-surface-container-lowest rounded-2xl p-md border border-outline-variant/30 shadow-sm flex flex-col gap-sm"
          >
            <h3 className="font-label-md text-on-surface">{t('editor.addSquiggle')}</h3>
            <div className="grid grid-cols-4 gap-sm">
              <label className="font-label-sm text-on-surface-variant flex flex-col gap-xs">
                {t('editor.line')}
                <input
                  type="number"
                  min={0}
                  value={newLine}
                  onChange={(e) => setNewLine(Number(e.target.value) || 0)}
                  className="font-mono font-label-md bg-surface-container-low text-on-surface border border-outline-variant/40 rounded-lg px-sm py-xs focus:outline-none focus-visible:ring-2 focus-visible:ring-primary/30"
                />
              </label>
              <label className="font-label-sm text-on-surface-variant flex flex-col gap-xs">
                {t('editor.startChar')}
                <input
                  type="number"
                  min={0}
                  value={newStartChar}
                  onChange={(e) => setNewStartChar(Number(e.target.value) || 0)}
                  className="font-mono font-label-md bg-surface-container-low text-on-surface border border-outline-variant/40 rounded-lg px-sm py-xs focus:outline-none focus-visible:ring-2 focus-visible:ring-primary/30"
                />
              </label>
              <label className="font-label-sm text-on-surface-variant flex flex-col gap-xs">
                {t('editor.endChar')}
                <input
                  type="number"
                  min={0}
                  value={newEndChar}
                  onChange={(e) => setNewEndChar(Number(e.target.value) || 0)}
                  className="font-mono font-label-md bg-surface-container-low text-on-surface border border-outline-variant/40 rounded-lg px-sm py-xs focus:outline-none focus-visible:ring-2 focus-visible:ring-primary/30"
                />
              </label>
              <label className="font-label-sm text-on-surface-variant flex flex-col gap-xs">
                {t('editor.severity')}
                <select
                  value={newSeverity}
                  onChange={(e) =>
                    setNewSeverity(e.target.value as EditorDiagnostic['severity'])
                  }
                  className="font-label-md bg-surface-container-low text-on-surface border border-outline-variant/40 rounded-lg px-sm py-xs focus:outline-none focus-visible:ring-2 focus-visible:ring-primary/30"
                >
                  {SEVERITIES.map((s) => (
                    <option key={s} value={s}>
                      {t(`editor.severity.${s}`)}
                    </option>
                  ))}
                </select>
              </label>
            </div>
            <label className="font-label-sm text-on-surface-variant flex flex-col gap-xs">
              {t('editor.message')}
              <input
                type="text"
                value={newMessage}
                onChange={(e) => setNewMessage(e.target.value)}
                placeholder={t('editor.message.placeholder')}
                className="font-label-md bg-surface-container-low text-on-surface border border-outline-variant/40 rounded-lg px-sm py-xs focus:outline-none focus-visible:ring-2 focus-visible:ring-primary/30"
              />
            </label>
            <button
              type="submit"
              disabled={!newMessage.trim()}
              className="self-start font-label-md bg-primary text-on-primary rounded-lg px-md py-sm cursor-pointer disabled:opacity-40 disabled:cursor-not-allowed hover:bg-primary/90 focus:outline-none focus-visible:ring-2 focus-visible:ring-primary/30"
            >
              {t('editor.addSquiggleBtn')}
            </button>
          </form>

          {diagCount > 0 ? (
            <div className="bg-surface-container-lowest rounded-2xl p-md border border-outline-variant/30 shadow-sm">
              <h3 className="font-label-md text-on-surface mb-sm">{t('editor.diagnosticsList')}</h3>
              <ul className="flex flex-col gap-xs">
                {diags.map((d, i) => (
                  <li key={i}>
                    <button
                      type="button"
                      onClick={() => onSquiggleClick(d)}
                      className="w-full text-left flex items-start gap-sm px-sm py-sm rounded-lg border border-outline-variant/30 bg-surface-container-low text-on-surface hover:bg-surface-container-high focus:outline-none focus-visible:ring-2 focus-visible:ring-primary/30 cursor-pointer"
                    >
                      <span
                        className="font-label-sm uppercase text-[10px] mt-0.5 tracking-wider"
                        style={{
                          color:
                            d.severity === 'error'
                              ? 'var(--color-error, #b3261e)'
                              : d.severity === 'warning'
                                ? 'var(--color-warning, #7c5800)'
                                : 'var(--color-on-surface-variant)',
                        }}
                      >
                        {t(`editor.severity.${d.severity}`)}
                      </span>
                      <span className="flex-1 font-label-md">
                        <span className="font-mono text-on-surface-variant">
                          {d.start_line + 1}:{d.start_character + 1}
                        </span>{' '}
                        {d.message}
                      </span>
                      {d.kind === 'auto' ? (
                        <span
                          className="font-label-sm uppercase text-[10px] tracking-wider text-on-surface-variant"
                          title={
                            d.source
                              ? (d.code
                                  ? intl.formatMessage({ id: 'editor.sourceTitle' }, { source: d.source, code: d.code })
                                  : intl.formatMessage({ id: 'editor.sourceTitle.noCode' }, { source: d.source }))
                              : t('editor.source')
                          }
                        >
                          {d.source ?? t('editor.source')}
                        </span>
                      ) : (
                        <span className="font-label-sm uppercase text-[10px] tracking-wider text-on-surface-variant">
                          {t('editor.manual')}
                        </span>
                      )}
                      <span className="material-symbols-outlined text-[14px] text-primary">
                        build
                      </span>
                    </button>
                    <button
                      type="button"
                      onClick={() => onAskAi(d)}
                      aria-label={t('editor.askAi')}
                      title={t('editor.askAi')}
                      className="flex items-center gap-xs px-xs py-0.5 rounded-full border border-outline-variant/40 bg-surface-container-low text-on-surface hover:bg-surface-container-high focus:outline-none focus-visible:ring-2 focus-visible:ring-primary/30 cursor-pointer"
                    >
                      <span className="material-symbols-outlined text-[14px] text-primary">
                        chat
                      </span>
                      <span className="font-label-sm">{t('editor.askAi')}</span>
                    </button>
                  </li>
                ))}
              </ul>
            </div>
          ) : null}
        </>
      ) : null}

      {drawer ? (
        <div
          className="fixed inset-0 z-[80] flex"
          role="dialog"
          aria-label={t('editor.quickFixDrawer')}
        >
          <button
            type="button"
            onClick={() => setDrawer(null)}
            aria-label={t('editor.closeDrawer')}
            className="flex-1 bg-black/30"
          />
          <aside ref={drawerRef} className="w-[420px] max-w-[90vw] bg-surface-container-lowest h-full overflow-auto p-md border-l border-outline-variant/30 shadow-lg flex flex-col gap-sm">
            <LspQuickFixPanel
              diagnostic={drawer}
              onApplied={() => {
                /* nothing — panel shows its own confirmation */
              }}
              onClose={() => setDrawer(null)}
            />
          </aside>
        </div>
      ) : null}
    </div>
  )
}
