// ReviewFileList — the review workbench's changed-file sidebar (Aurora 2026-10,
// 03 页 P1): the session's changed-file list (FileDiffList) whose per-file
// status badges stay live with the review body's a/r/u decisions via the
// lifted decisionsByPath map.
//
// T7 (03 review-wrap): extracted from RightDock's DockDiffSidebar unchanged —
// the same list now serves BOTH hosts, the dock's Diff summary tab and the
// main-area ReviewWorkspace (the review body moved there; see
// ReviewWorkspace.tsx).

import { useEffect, useRef, useState } from 'react'
import FileDiffList, { type FileFilter } from '@/components/diff/FileDiffList'
import * as api from '@/lib/tauri-api'
import type { HunkDecision } from '@/lib/diff-merge'
import type { FileDiff } from '@/types'

interface ReviewFileListProps {
  files: string[]
  currentPath: string | null
  decisionsByPath: Map<string, Map<string, HunkDecision>>
  onSelectPath?: (path: string) => void
  /** FileDiffList defaults to w-64; the dock's summary tab passes w-72. */
  className?: string
}

export default function ReviewFileList({
  files,
  currentPath,
  decisionsByPath,
  onSelectPath,
  className,
}: ReviewFileListProps) {
  const [diffs, setDiffs] = useState<Map<string, FileDiff>>(new Map())
  const [filter, setFilter] = useState<FileFilter>('all')
  const fetchedRef = useRef<Set<string>>(new Set())

  useEffect(() => {
    let cancelled = false
    for (const path of files) {
      if (fetchedRef.current.has(path)) continue
      fetchedRef.current.add(path)
      api.getFileDiff(path)
        .then(d => { if (!cancelled) setDiffs(prev => new Map(prev).set(path, d)) })
        .catch(() => {
          // Diff unavailable (binary / deleted / demo mode): the row still
          // renders, just without +/− counts.
        })
    }
    return () => { cancelled = true }
  }, [files])

  return (
    <FileDiffList
      files={files}
      diffs={diffs}
      decisions={decisionsByPath}
      currentPath={currentPath}
      filter={filter}
      onSelectPath={path => onSelectPath?.(path)}
      onFilterChange={setFilter}
      className={className}
    />
  )
}
