// ChatStatusBar — Aurora redesign 2026-10 (docs/design/ui-redesign-2026-10/
// pages/02-chat.html 状态条), slimmed by the 02c composer 收敛裁决 (D5,
// 2026-10-14): the strip carries ONLY
//   * working directory — the session's `working_dir`, doubling as the app's
//     only WD-picker entry point (same button, aria label, breadcrumb format
//     and disabled-without-session behavior as always);
//   * git branch — `current_git_branch` IPC on workingDir change (per-dir
//     memo); hides for non-git dirs and while the lookup is in flight (a
//     stale branch never renders).
//
// The former context / cache-hit / spend / budget segments moved into the
// composer's usage dialog (SessionUsageDialog, opened from the context-%
// chip beside the model chip) per 裁决 D4 — the status bar no longer shows
// numbers or money. Each remaining segment still hides independently when
// its source is absent; nothing is ever mocked up to fill the line.

import { useEffect, useRef, useState } from 'react'
import { Button } from '@/components/ui/button'
import { useT } from '@/i18n'
import { formatDirBreadcrumb } from '@/pages/chat/utils'
import * as api from '@/lib/tauri-api'

export interface ChatStatusBarProps {
  /** Session working dir (may be empty → "not set" segment). */
  workingDir: string
  /** Current session id — gates the WD-picker button; null → "Not set". */
  sessionId: string | null
  /** Working-directory picker action (ComposerPanel owns the shortcut). */
  onChangeWorkingDir: () => void
}

export function ChatStatusBar({ workingDir, sessionId, onChangeWorkingDir }: ChatStatusBarProps) {
  const t = useT()

  // Branch segment — one IPC per distinct working dir (memoized for the
  // session-switch back-and-forth); the segment hides for non-git dirs and
  // while the lookup is in flight (a stale branch never renders).
  const [branch, setBranch] = useState<string | null>(null)
  const branchCacheRef = useRef<Map<string, string | null>>(new Map())
  useEffect(() => {
    if (!workingDir) {
      setBranch(null)
      return
    }
    const cached = branchCacheRef.current.get(workingDir)
    if (cached !== undefined) {
      setBranch(cached)
      return
    }
    let cancelled = false
    void api.currentGitBranch(workingDir).then((b) => {
      branchCacheRef.current.set(workingDir, b)
      if (!cancelled) setBranch(b)
    }).catch(() => {
      branchCacheRef.current.set(workingDir, null)
      if (!cancelled) setBranch(null)
    })
    return () => { cancelled = true }
  }, [workingDir])

  return (
    // G1-A parity batch (2026-10-15, design 02c D5 状态条): the line renders
    // at the design's density — 26px tall, mono 11px labels, t3-muted color,
    // hairline on the TOP edge. The directory + branch segments and their
    // logic are unchanged.
    <div
      data-testid="chat-status-bar"
      aria-label={t('chat.statusbar.aria')}
      className="mt-xs flex items-center gap-md border-t border-outline-variant/20 px-sm h-[26px] font-mono text-label-xs text-muted-foreground min-w-0"
    >
      {/* Working directory — the WD-picker entry. Always renders (set or
          "not set" — both real). */}
      <Button
        type="button"
        variant="ghost"
        onClick={onChangeWorkingDir}
        disabled={!sessionId}
        className="flex items-center gap-xs min-w-0 max-w-[50%] h-[22px] px-xs rounded-sm hover:text-primary transition-colors disabled:opacity-50 disabled:cursor-not-allowed font-label-xs"
        title={workingDir || t('chat.input.footer.workingDir.unset')}
        aria-label={t('chat.input.footer.workingDir.aria')}
      >
        <span className="material-symbols-outlined icon-xs shrink-0">folder</span>
        <span className="truncate font-mono">
          {workingDir ? formatDirBreadcrumb(workingDir) : t('chat.input.footer.workingDir.unset')}
        </span>
      </Button>

      {branch != null && (
        <span
          data-testid="chat-status-branch"
          className="flex items-center gap-xs shrink-0 font-mono min-w-0"
          title={t('chat.statusbar.branch.title')}
        >
          <span className="material-symbols-outlined icon-xs shrink-0" aria-hidden="true">account_tree</span>
          <span className="truncate">{branch}</span>
        </span>
      )}
    </div>
  )
}

export default ChatStatusBar
