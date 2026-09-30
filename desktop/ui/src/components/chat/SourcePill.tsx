// SourcePill — office Wave 3 C8: the citation-tracing pill for B3-injected
// source lines.
//
// Convention (B3 v1 injection): a whole line of the shape
//   [Source: <name>] (<url-or-path>)
// renders as one compact pill (name + link glyph). Clicking it opens the
// target where it lives: http(s) through the external-browser wrapper,
// anything else (file path) through the OS default app. The pill is purely a
// viewer affordance — it never mutates the markdown around it.

import { toastError } from '@/lib/errorToast'
import { openExternal, openWithDefaultApp } from '@/lib/tauri-api'
import { useT } from '@/i18n'

/** A parsed B3 source line. */
export interface SourceRef {
  name: string
  target: string
}

/**
 * Whole-line matcher for the B3 injection convention. Returns null for
 * anything else — partial lines, mentions inside sentences and code fences
 * (which never reach the paragraph renderer) stay literal markdown.
 */
export function matchSourceLine(line: string): SourceRef | null {
  const m = /^\s*\[Source:\s*(.+?)\]\s*\((.+?)\)\s*$/.exec(line)
  return m ? { name: m[1].trim(), target: m[2].trim() } : null
}

export interface SourcePillProps {
  name: string
  target: string
}

export function SourcePill({ name, target }: SourcePillProps) {
  const t = useT()
  const open = () => {
    const opener = /^https?:\/\//i.test(target) ? openExternal(target) : openWithDefaultApp(target)
    opener.catch((err) => toastError(t('link.open.failed'), err))
  }

  return (
    <span
      data-testid="source-pill"
      className="inline-flex max-w-full items-center gap-xs rounded-full bg-secondary-container/60 border border-outline-variant/20 px-sm py-[2px] font-label-xs text-on-surface align-middle"
    >
      <span className="material-symbols-outlined icon-xs text-primary shrink-0" aria-hidden="true">link</span>
      <button
        type="button"
        data-testid="source-pill-open"
        title={t('chat.source.view')}
        aria-label={`${t('chat.source.view')}: ${name}`}
        onClick={open}
        className="min-w-0 truncate font-label-xs text-on-surface hover:text-primary focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary/30 rounded-full cursor-pointer"
      >
        {name}
      </button>
    </span>
  )
}

export default SourcePill
