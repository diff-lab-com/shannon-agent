// Editor page — load a source file, render it with CodeMirror, auto-fetch
// LSP diagnostics, and let the user add manual diagnostic squiggles too.
// Clicking a squiggle opens the LspQuickFixPanel in a side drawer.
//
// Phase E1 v2: auto-LSP diagnostics via publishDiagnostics subscription.
// Phase E1 v1: manual squiggle UI.
//
// Orchestrator-only: all sub-components live under ./editor/. State and
// callbacks stay here so the page is a single source of truth.

import { useCallback, useEffect, useRef, useState } from 'react'
import { useNavigate } from 'react-router-dom'
import { useT } from '@/i18n'
import { open as openDialog } from '@tauri-apps/plugin-dialog'
import { toast } from 'sonner'
import CodeEditor, {
  type EditorDiagnostic,
} from '@/components/editor/CodeEditor'
import { ConfirmDialog } from '@/components/ui/confirm-dialog'
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

type EditorProps = {
  /** P0-B: pre-load this file (chat file-ref chips deep-link into the editor). */
  initialPath?: string | null
  /** B0 P0-5: report `draft !== file.content` so the host can guard closing. */
  onDirtyChange?: (dirty: boolean) => void
}

export default function Editor({ initialPath, onDirtyChange }: EditorProps) {
  const t = useT()
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

  // B0 P0-5: switching files with unsaved edits asks before discarding.
  const [pendingLoadPath, setPendingLoadPath] = useState<string | null>(null)

  // Side drawer for quick-fix
  const [drawer, setDrawer] = useState<DrawerDiag | null>(null)

  // 34: race guard for diagnostics responses — `loadPath` may fire a second
  // fetch (or the user re-runs) before the first resolves; the stale
  // response must not overwrite the fresh one.
  const diagRequestIdRef = useRef(0)

  const fetchDiagnostics = useCallback(async (sourceFile: SourceFile) => {
    const requestId = ++diagRequestIdRef.current
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
      if (diagRequestIdRef.current !== requestId) return // superseded
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
      if (diagRequestIdRef.current !== requestId) return // superseded
      setAutoDiags([])
      setDiagError(String(err))
    } finally {
      if (diagRequestIdRef.current === requestId) setDiagLoading(false)
    }
  }, [])

  const loadPath = useCallback(
    async (target: string) => {
      if (!target.trim()) return
      setLoading(true)
      setLoadError(null)
      try {
        const dto = await api.readSourceFile(target.trim())
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
    [fetchDiagnostics],
  )

  const onLoad = useCallback(
    async (e: React.FormEvent) => {
      e.preventDefault()
      const target = filePath.trim()
      if (!target) return
      // B0 P0-5: dirty draft → confirm the discard before loading another file.
      if (file != null && draft !== file.content) {
        setPendingLoadPath(target)
        return
      }
      await loadPath(target)
    },
    [filePath, file, draft, loadPath],
  )

  // P0-B: deep-link support — load the chip's file once on mount (and when
  // a new chip targets a different file while the panel is open).
  useEffect(() => {
    if (!initialPath) return
    setFilePath(initialPath)
    void loadPath(initialPath)
  }, [initialPath, loadPath])

  // B0 P0-5: unsaved-edit detector. `draft` is reset whenever a file loads
  // or a save lands, so `draft !== file.content` is exactly "the user typed
  // something they have not saved yet". The host (Chat's inline panel modal)
  // gates closing on this.
  useEffect(() => {
    onDirtyChange?.(file != null && draft !== file.content)
  }, [file, draft, onDirtyChange])

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

  // P1-35: a quick fix rewrites the file on disk. If the editor kept its
  // (now stale) draft, the next save would clobber the fix — so re-read the
  // file as soon as a fix applies. When the draft was dirty, the disk
  // content wins and the user is told their unsaved edits were replaced.
  const onQuickFixApplied = useCallback(() => {
    if (!file) return
    if (draft !== file.content) {
      toast.info(t('editor.quickFix.reloadDirty'))
    }
    void loadPath(file.path)
  }, [file, draft, loadPath, t])

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
        <QuickFixDrawer
          t={t}
          drawer={drawer}
          onApplied={onQuickFixApplied}
          onClose={() => setDrawer(null)}
        />
      ) : null}

      <ConfirmDialog
        open={pendingLoadPath !== null}
        title={t('editor.discard.title')}
        message={t('editor.discard.message')}
        confirmLabel={t('editor.discard.confirm')}
        cancelLabel={t('editor.discard.cancel')}
        destructive
        onConfirm={() => {
          const target = pendingLoadPath
          setPendingLoadPath(null)
          if (target) void loadPath(target)
        }}
        onCancel={() => setPendingLoadPath(null)}
      />
    </div>
  )
}