// WebRenderer — the dock web tab's inline browser view
// (docs/plans/2026-09-25-desktop-chat-ui-open-and-artifact-design.md §4 P1-E).
//
// Decision §5-6: external links default here. The sandbox keeps the frame
// opaque (no same-origin, no top-navigation); the CSP `frame-src` change in
// tauri.conf.json authorizes the embed itself. X-Frame-Options /
// frame-ancestors rejections are undetectable from inside a cross-origin
// iframe, so a server-side probe (`probe_url_frameable`) decides when to
// show the fallback card, with a loading timeout as the backstop.

import { useCallback, useEffect, useRef, useState } from 'react'
import { useIntl } from 'react-intl'
import { openExternal, probeUrlFrameable } from '@/lib/tauri-api'

const FRAME_TIMEOUT_MS = 12_000

type Phase = 'loading' | 'loaded' | 'blocked'

export function WebRenderer({ url }: { url: string }) {
  const intl = useIntl()
  const t = useCallback((id: string) => intl.formatMessage({ id }), [intl])
  const [phase, setPhase] = useState<Phase>('loading')
  const [frameKey, setFrameKey] = useState(0)
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null)

  useEffect(() => {
    setPhase('loading')
    let alive = true
    // Server-side framing probe — turns known-blocked sites into an
    // immediate fallback card instead of a blank frame.
    probeUrlFrameable(url)
      .then(p => {
        if (alive && !p.frameable) setPhase('blocked')
      })
      .catch(() => {
        /* probe failure: let the frame try */
      })
    timerRef.current = setTimeout(() => {
      if (alive) setPhase(p => (p === 'loading' ? 'blocked' : p))
    }, FRAME_TIMEOUT_MS)
    return () => {
      alive = false
      if (timerRef.current) clearTimeout(timerRef.current)
    }
  }, [url, frameKey])

  const onFrameLoad = useCallback(() => {
    // XFO error pages also fire `load` — only count it as success while the
    // probe hasn't already ruled the site blocked.
    setPhase(p => (p === 'loading' ? 'loaded' : p))
  }, [])

  const itemClass =
    'flex items-center gap-xs px-sm py-xs rounded-lg text-on-surface-variant hover:bg-surface-container hover:text-on-surface gap-0 px-sm h-auto py-xs'

  return (
    <div className="relative flex flex-col h-full min-h-0 rounded-lg overflow-hidden border border-outline-variant/15 bg-white">
      {/* Address bar — display-only by design (§4 P1-E ④): the panel is a
          reader, not a general-purpose browser. */}
      <div className="flex items-center gap-xs px-sm py-xs bg-surface-container-low border-b border-outline-variant/15 shrink-0">
        <span className="material-symbols-outlined icon-sm text-on-surface-variant shrink-0" aria-hidden="true">language</span>
        <span className="font-mono text-[11px] text-on-surface-variant truncate flex-1 min-w-0" title={url}>
          {url}
        </span>
        <button
          type="button"
          className={itemClass}
          aria-label={t('chat.dock.web.reload')}
          title={t('chat.dock.web.reload')}
          onClick={() => setFrameKey(k => k + 1)}
        >
          <span className="material-symbols-outlined icon-sm" aria-hidden="true">refresh</span>
        </button>
        <button
          type="button"
          className={itemClass}
          aria-label={t('link.menu.openBrowser')}
          title={t('link.menu.openBrowser')}
          onClick={() => void openExternal(url)}
        >
          <span className="material-symbols-outlined icon-sm" aria-hidden="true">open_in_new</span>
        </button>
      </div>
      <div className="relative flex-1 min-h-0">
        <iframe
          key={frameKey}
          title={url}
          src={url}
          onLoad={onFrameLoad}
          // Opaque origin: scripts run, forms submit, but the frame can
          // neither read our DOM/storage nor navigate the app window.
          sandbox="allow-scripts allow-forms allow-popups"
          referrerPolicy="no-referrer"
          className="w-full h-full border-0 bg-white"
        />
        {phase === 'blocked' && (
          <div className="absolute inset-0 flex flex-col items-center justify-center gap-sm p-lg text-center bg-surface-container-lowest">
            <span className="material-symbols-outlined icon-md text-on-surface-variant/60" aria-hidden="true">block</span>
            <p className="font-label-lg text-on-surface">{t('chat.dock.web.frameBlocked')}</p>
            <p className="font-label-sm text-on-surface-variant max-w-sm">{t('chat.dock.web.frameBlockedHint')}</p>
            <button
              type="button"
              data-testid="web-frame-blocked-open"
              className="mt-xs px-md py-xs rounded-lg bg-primary text-on-primary font-label-md hover:bg-primary/90 cursor-pointer"
              onClick={() => void openExternal(url)}
            >
              {t('link.menu.openBrowser')}
            </button>
          </div>
        )}
      </div>
    </div>
  )
}
