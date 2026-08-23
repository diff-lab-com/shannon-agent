// Editor page — load a source file, render it with CodeMirror, auto-fetch
// LSP diagnostics, and let the user add manual diagnostic squiggles too.
// Clicking a squiggle opens the LspQuickFixPanel in a side drawer.
//
// Phase E1 v2: auto-LSP diagnostics via publishDiagnostics subscription.
// Phase E1 v1: manual squiggle UI.
//
// Orchestrator-only: all sub-components live under ./editor/. State and
// callbacks stay here so the page is a single source of truth.

import { useCallback, useState } from 'react'
import { useNavigate } from 'react-router-dom'
import { useIntl, type PrimitiveType } from 'react-intl'
import { open as openDialog } from '@tauri-apps/plugin-dialog'
import { toast } from 'sonner'
import CodeEditor, {
  type EditorDiagnostic,
} from '@/components/editor/CodeEditor'
import * as api from '@/lib/tauri-api'
import type { SourceFile } from '@/lib/tauri-api'
import {
  AddSquiggleForm,
  DiagBanner,
  DiagList,
  EditorToolbar,
  FileLoadForm,
  QuickFixDrawer,
  normalizeSeverity,
} from './editor'
import type { AutoDiagnostic, DrawerDiag, ManualDiagnostic, MixedDiagnostic } from './editor'

export default function Editor() {
  const intl = useIntl()
  const t = (id: string, values?: Record<string, PrimitiveType>) => intl.formatMessage({ id }, values)
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

      <FileLoadForm
        t={t}
        filePath={filePath}
        setFilePath={setFilePath}
        loading={loading}
        loadError={loadError}
        onLoad={onLoad}
        onBrowse={onBrowse}
      />

      {file ? (
        <>
          <EditorToolbar
            t={t}
            file={file}
            diagCount={diagCount}
            diagLoading={diagLoading}
            editMode={editMode}
            saving={saving}
            fetchDiagnostics={fetchDiagnostics}
            onToggleEdit={onToggleEdit}
            onSave={onSave}
          />

          <DiagBanner
            t={t}
            diagError={diagError}
            diagTimedOut={diagTimedOut}
          />

          <CodeEditor
            value={editMode ? draft : file.content}
            onValueChange={editMode ? setDraft : undefined}
            language={file.language_id}
            diagnostics={diags}
            onDiagnosticClick={onSquiggleClick}
            readOnly={!editMode}
          />

          <AddSquiggleForm
            t={t}
            newLine={newLine}
            setNewLine={setNewLine}
            newStartChar={newStartChar}
            setNewStartChar={setNewStartChar}
            newEndChar={newEndChar}
            setNewEndChar={setNewEndChar}
            newMessage={newMessage}
            setNewMessage={setNewMessage}
            newSeverity={newSeverity}
            setNewSeverity={setNewSeverity}
            onAddSquiggle={onAddSquiggle}
          />

          <DiagList
            t={t}
            diags={diags}
            onSquiggleClick={onSquiggleClick}
            onAskAi={onAskAi}
          />
        </>
      ) : null}

      {drawer ? (
        <QuickFixDrawer t={t} drawer={drawer} onClose={() => setDrawer(null)} />
      ) : null}
    </div>
  )
}