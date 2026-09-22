// Batch D1 (2026-09-20 delta analysis): the right-dock document TOC — the
// ZCode 阅读器 pattern. Numbered heading rail (h1–h3) with scroll-spy
// highlighting and click-to-scroll, sticky inside the dock's scroll panel;
// below the min-heading threshold it renders nothing.
//
// The scroll root is the dock tabpanel (`#dock-tabpanel` in RightDock) —
// the TOC rail lives inside that scrolling ancestor and positions its
// sticky column against it.

import { useEffect, useMemo, useState } from 'react'
import { useT } from '@/i18n'
import { cn } from '@/lib/utils'
import { parseDocHeadings } from './docToc'

const MIN_HEADINGS = 3
const SCROLL_ROOT_ID = 'dock-tabpanel'

export function DocumentToc({ source }: { source: string }) {
  const t = useT()
  const headings = useMemo(() => parseDocHeadings(source), [source])
  const [activeId, setActiveId] = useState<string | null>(null)

  useEffect(() => {
    if (headings.length < MIN_HEADINGS) return
    const root = document.getElementById(SCROLL_ROOT_ID)
    if (!root) return

    const update = () => {
      const rootTop = root.getBoundingClientRect().top
      let current: string | null = null
      for (const h of headings) {
        const el = document.getElementById(h.id)
        if (!el) continue
        // The active heading is the last one at or above the reading line
        // (a quarter into the viewport) — matches reader-app scroll-spy.
        if (el.getBoundingClientRect().top - rootTop <= root.clientHeight * 0.25) {
          current = h.id
        } else {
          break
        }
      }
      setActiveId(prev => (prev === current ? prev : current))
    }
    update()
    root.addEventListener('scroll', update, { passive: true })
    return () => root.removeEventListener('scroll', update)
  }, [headings])

  if (headings.length < MIN_HEADINGS) return null

  const jump = (id: string) => {
    document.getElementById(id)?.scrollIntoView({ behavior: 'smooth', block: 'start' })
  }

  return (
    <nav
      aria-label={t('chat.artifact.toc.aria')}
      data-testid="document-toc"
      className="w-40 shrink-0 hidden lg:block"
    >
      {/* Sticky within the scrolling tabpanel — stays visible while the
          document body scrolls, like the reference reader's TOC rail. */}
      <div className="sticky top-0 max-h-full overflow-y-auto py-sm pl-sm border-l border-outline-variant/15">
        <p className="font-label-xs uppercase tracking-wider text-on-surface-variant mb-xs px-sm">
          {t('chat.artifact.toc.title')}
        </p>
        <ol className="space-y-0.5">
          {headings.map((h, i) => (
            <li key={h.id}>
              <button
                type="button"
                onClick={() => jump(h.id)}
                aria-current={activeId === h.id ? 'location' : undefined}
                className={cn(
                  'w-full flex items-start gap-1.5 text-left px-sm py-1 rounded-md font-label-sm transition-colors cursor-pointer',
                  h.level === 1 ? 'font-bold' : h.level === 3 ? 'pl-md' : '',
                  activeId === h.id
                    ? 'bg-primary/10 text-primary'
                    : 'text-on-surface-variant hover:bg-surface-container-low hover:text-on-surface',
                )}
              >
                <span className="font-mono text-[10px] tabular-nums mt-[2px] shrink-0" aria-hidden="true">
                  {i + 1}
                </span>
                <span className="truncate flex-1 min-w-0">{h.text}</span>
              </button>
            </li>
          ))}
        </ol>
      </div>
    </nav>
  )
}
