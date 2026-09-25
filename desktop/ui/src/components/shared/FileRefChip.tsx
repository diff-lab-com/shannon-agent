// FileRefChip — a clickable file path inside chat content / tool input
// (docs/plans/2026-09-25-desktop-chat-ui-open-and-artifact-design.md §4 P0-B).
//
// The chip only becomes interactive after the backend existence probe
// confirms the path (§5-4: hallucinated paths must not look clickable);
// until then — and forever after a "missing" verdict — it renders as the
// exact same inline-code style the reader already knows. Right-click offers
// the three file actions; plain click takes the app's best default surface.

import { useEffect, useMemo, useState } from 'react'
import { useIntl } from 'react-intl'
import { toast } from 'sonner'
import { cn } from '@/lib/utils'
import { messageFor, useT } from '@/i18n'
import {
  basenameOf,
  getActiveWorkingDir,
  looksLikeFilePath,
  resolveFileRefPath,
} from '@/lib/fileRefs'
import { openFileRef } from '@/lib/openFileRef'
import { openWithDefaultApp, pathExists, revealInFolder } from '@/lib/tauri-api'
import { toastError } from '@/lib/errorToast'

const INLINE_CODE_FALLBACK_CLASS =
  'font-mono text-[0.92em] px-[5px] py-[1px] rounded-md bg-surface-container text-primary border border-outline-variant/15'

interface FileRefChipProps {
  raw: string
  className?: string
}

export function FileRefChip({ raw, className }: FileRefChipProps) {
  const t = useT()
  const intl = useIntl()
  // Resolve against the module-level working dir (synced by Chat) — keeps
  // Markdown's component tree free of prop drilling.
  const absPath = useMemo(
    () => (looksLikeFilePath(raw) ? resolveFileRefPath(raw, getActiveWorkingDir()) : null),
    // The active working dir is a module-level ref (synced by Chat), not
    // reactive state — reading it once per raw token is the contract.
    [raw],
  )
  const [exists, setExists] = useState<boolean | null>(null)
  const [menu, setMenu] = useState<{ x: number; y: number } | null>(null)

  useEffect(() => {
    if (!absPath) {
      setExists(null)
      return
    }
    let alive = true
    setExists(null)
    pathExists(absPath)
      .then((v) => {
        if (alive) setExists(v)
      })
      .catch(() => {
        if (alive) setExists(null)
      })
    return () => {
      alive = false
    }
  }, [absPath])

  if (!absPath || exists === false) {
    return <code className={cn(INLINE_CODE_FALLBACK_CLASS, className)}>{raw}</code>
  }

  const interactive = exists === true
  const baseName = basenameOf(absPath)

  const handleOpen = () => {
    openFileRef(absPath)
  }

  const withPathErrors = (action: (p: string) => Promise<unknown>) => {
    void action(absPath).catch((e) => toastError(t('link.open.failed'), e))
  }

  const itemClass =
    'flex w-full items-center gap-xs px-sm py-xs text-left font-label-md text-on-surface hover:bg-surface-container rounded-md cursor-pointer whitespace-nowrap'

  return (
    <span className="relative inline-flex items-baseline">
      <button
        type="button"
        disabled={!interactive}
        onClick={handleOpen}
        onContextMenu={(e) => {
          e.preventDefault()
          setMenu({ x: e.clientX, y: e.clientY })
        }}
        title={absPath}
        aria-label={intl.formatMessage({ id: 'link.fileRef.exists.aria' }, { path: baseName })}
        data-testid="file-ref-chip"
        className={cn(
          'inline-flex items-baseline gap-[3px] font-mono text-[0.92em] px-[5px] py-[1px] rounded-md border transition-colors',
          interactive
            ? 'bg-primary-container/25 text-primary border-primary/25 hover:bg-primary-container/50 hover:border-primary/50 cursor-pointer'
            : 'bg-surface-container text-on-surface-variant border-outline-variant/15 cursor-progress',
          className,
        )}
      >
        <span className="material-symbols-outlined text-[12px] leading-none translate-y-[1px]" aria-hidden="true">
          description
        </span>
        {raw}
      </button>
      {menu && (
        <>
          <div className="fixed inset-0 z-modal" onClick={() => setMenu(null)} onContextMenu={(e) => { e.preventDefault(); setMenu(null) }} />
          <div
            role="menu"
            aria-label={messageFor('link.fileRef.menu.aria', { path: baseName })}
            className="fixed z-modal min-w-44 rounded-lg border border-outline-variant/20 bg-surface-container-high p-xs shadow-lg animate-in fade-in zoom-in-95"
            style={{
              left: Math.max(4, Math.min(menu.x, window.innerWidth - 200)),
              top: Math.max(4, Math.min(menu.y, window.innerHeight - 130)),
            }}
          >
            <button type="button" role="menuitem" className={itemClass} onClick={() => { setMenu(null); handleOpen() }}>
              <span className="material-symbols-outlined text-[16px]" aria-hidden="true">chat_info</span>
              {t('link.fileRef.menu.open')}
            </button>
            <button type="button" role="menuitem" className={itemClass} onClick={() => { setMenu(null); withPathErrors(revealInFolder) }}>
              <span className="material-symbols-outlined text-[16px]" aria-hidden="true">folder_open</span>
              {t('link.fileRef.menu.reveal')}
            </button>
            <button type="button" role="menuitem" className={itemClass} onClick={() => { setMenu(null); withPathErrors(openWithDefaultApp) }}>
              <span className="material-symbols-outlined text-[16px]" aria-hidden="true">open_in_new</span>
              {t('link.fileRef.menu.system')}
            </button>
          </div>
        </>
      )}
    </span>
  )
}

/** Shared "missing file" toast for hosts that open paths directly. */
export function toastMissingFile(path: string): void {
  toast.error(messageFor('link.fileRef.missing', { path: basenameOf(path) }))
}
