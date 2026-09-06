/**
 * P1-5 C-2 — diff panel content: the session's uncommitted changes.
 *
 * Reuses the `get_session_git_diff` data flow (the /diff slash backend):
 * numstat file list on the left, the selected file rendered read-only
 * through the existing DiffViewer (no hunk decisions in a panel context).
 * File bodies come from `get_file_diff` with an absolute path (session
 * working dir + repo-relative numstat path) — the same call DiffDialog
 * uses. Not-a-repo / no-changes surface as calm empty states.
 */
import { useCallback, useEffect, useMemo, useState } from 'react'
import { Spinner } from '@/components/ui/loading-state'
import DiffViewer from '@/components/diff/DiffViewer'
import { useT } from '@/i18n'
import * as api from '@/lib/tauri-api'
import type { GitDiffSummary } from '@/lib/tauri-api'
import type { FileDiff } from '@/types'
import { cn } from '@/lib/utils'

interface SessionDiffPanelProps {
  /** Session working directory (repo root or a dir inside it). */
  workingDir: string | null
}

/** numstat paths are repo-relative; the diff backend needs absolute ones. */
function toAbsolutePath(workingDir: string, repoPath: string): string {
  return `${workingDir.replace(/\/+$/, '')}/${repoPath.replace(/^\/+/, '')}`
}

export function SessionDiffPanel({ workingDir }: SessionDiffPanelProps) {
  const t = useT()
  const [summary, setSummary] = useState<GitDiffSummary | null>(null)
  const [summaryError, setSummaryError] = useState<string | null>(null)
  const [loading, setLoading] = useState(false)
  const [selectedPath, setSelectedPath] = useState<string | null>(null)
  const [fileDiff, setFileDiff] = useState<FileDiff | null>(null)
  const [fileError, setFileError] = useState(false)

  useEffect(() => {
    if (!workingDir) {
      setSummary(null)
      setSummaryError(null)
      return
    }
    let cancelled = false
    setLoading(true)
    setSummaryError(null)
    api.getSessionGitDiff(workingDir)
      .then(result => {
        if (cancelled) return
        setSummary(result)
        setSelectedPath(result.files[0]?.path ?? null)
      })
      .catch(e => { if (!cancelled) setSummaryError(e instanceof Error ? e.message : String(e)) })
      .finally(() => { if (!cancelled) setLoading(false) })
    return () => { cancelled = true }
  }, [workingDir])

  useEffect(() => {
    if (!selectedPath || !workingDir) {
      setFileDiff(null)
      setFileError(false)
      return
    }
    let cancelled = false
    setFileDiff(null)
    setFileError(false)
    api.getFileDiff(toAbsolutePath(workingDir, selectedPath))
      .then(diff => { if (!cancelled) setFileDiff(diff) })
      .catch(() => { if (!cancelled) setFileError(true) })
    return () => { cancelled = true }
  }, [selectedPath, workingDir])

  const select = useCallback((path: string) => setSelectedPath(path), [])

  const files = summary?.files ?? []
  const emptyState = useMemo(() => {
    if (loading) return null
    if (summaryError) return { icon: 'error', text: t('workspace.diff.loadFailed'), detail: summaryError }
    if (summary && !summary.is_repo) return { icon: 'folder_off', text: t('workspace.diff.notRepo') }
    if (files.length === 0) return { icon: 'check_circle', text: t('workspace.diff.empty') }
    return null
  }, [loading, summaryError, summary, files.length, t])

  return (
    <div
      role="region"
      aria-label={t('workspace.diff.title')}
      data-testid="session-diff-panel"
      className="flex h-full min-h-0 flex-col"
    >
      {emptyState ? (
        <div className="flex flex-1 flex-col items-center justify-center gap-sm p-md text-center">
          <span className="material-symbols-outlined icon-lg text-on-surface-variant" aria-hidden="true">{emptyState.icon}</span>
          <p className="font-body-md text-on-surface-variant max-w-64">{emptyState.text}</p>
          {emptyState.detail && <p className="font-body-sm text-on-surface-variant/70 break-all">{emptyState.detail}</p>}
        </div>
      ) : (
        <>
          <div
            role="tablist"
            aria-label={t('workspace.diff.files.aria')}
            className="shrink-0 overflow-x-auto border-b border-outline-variant/20 bg-surface-container-low/50 px-sm py-1"
          >
            {files.map(file => (
              <button
                key={file.path}
                type="button"
                role="tab"
                aria-selected={file.path === selectedPath}
                onClick={() => select(file.path)}
                title={file.path}
                className={cn(
                  'mr-xs inline-flex shrink-0 items-center gap-xs rounded-t-md px-sm py-1 font-label-sm text-label-sm',
                  file.path === selectedPath
                    ? 'bg-surface-container-lowest text-on-surface'
                    : 'text-on-surface-variant hover:bg-surface-container',
                )}
              >
                <span className="max-w-48 truncate">{file.path}</span>
                <span className="tabular-nums text-[11px]" aria-hidden="true">
                  <span className="text-success">+{file.insertions}</span>{' '}
                  <span className="text-error">-{file.deletions}</span>
                </span>
              </button>
            ))}
            {summary?.truncated && (
              <span className="font-label-xs text-on-surface-variant px-xs">{t('workspace.diff.truncated')}</span>
            )}
          </div>
          <div className="flex-1 overflow-auto min-h-0 p-sm">
            {fileError ? (
              <div className="flex h-full items-center justify-center gap-sm text-on-surface-variant">
                <span className="material-symbols-outlined icon-md" aria-hidden="true">error</span>
                <p className="font-body-sm">{t('workspace.diff.fileFailed')}</p>
              </div>
            ) : fileDiff ? (
              <DiffViewer diff={fileDiff} decisions={new Map()} onToggleHunk={() => {}} />
            ) : (
              <div className="flex h-full items-center justify-center">
                <Spinner className="text-primary" />
              </div>
            )}
          </div>
        </>
      )}
    </div>
  )
}
