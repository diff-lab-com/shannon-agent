import { useEffect, useMemo, useRef, useState } from 'react'
import { registerInteractiveHtml, unregisterInteractiveArtifact } from '@/lib/tauri-api'

interface HtmlRendererProps {
  source: string
  title?: string
  /**
   * 2026-09-26 round2 §5-1 A (decision A): serve the document from the
   * `artifact://` custom protocol instead of srcdoc. The artifact document
   * gets its own strict response CSP and runs in an opaque origin inside a
   * `allow-scripts allow-forms` sandbox — the only combination that can
   * really run scripts (srcdoc children inherit the app CSP, where
   * `script-src 'self'` kills every script). Only chat-fence HTML opts in;
   * disk files stay on the static path.
   */
  interactive?: boolean
  /**
   * Fired (once per failed registration) when the interactive path is
   * unavailable and the renderer silently fell back to the static srcDoc —
   * the dock uses it to surface the static hint again.
   */
  onRegistrationFailed?: () => void
}

// B3 §P1-9 "honest static" posture (decision §5-1, short term): srcdoc
// iframes inherit the parent CSP (`script-src 'self'` in production) and
// multiple policies intersect — an injected meta could never re-grant
// scripts, so `allow-scripts` here only ever produced a *silently static*
// page. The posture is now truthful: no script permission at all (the meta
// drops the dead script-src and keeps style/img allowances), and the dock
// surfaces a hint + the OS escape hatch instead of pretending otherwise.
// True interactive HTML now rides the `artifact://` custom protocol
// (`interactive` above); the static path below is byte-identical to the
// B3 implementation.
const STATIC_CSP = '<meta http-equiv="Content-Security-Policy" content="default-src \'none\'; style-src \'unsafe-inline\' \'unsafe-hashes\'; img-src data:; font-src data:;">'

// §5-1 A — the exact sandbox contract for interactive artifacts. Scripts
// and forms run; same-origin, top navigation, modals and popups do not
// (the document stays opaque-origin: no access to host DOM/storage).
const INTERACTIVE_SANDBOX = 'allow-scripts allow-forms'

export function HtmlRenderer({ source, title, interactive = false, onRegistrationFailed }: HtmlRendererProps) {
  const srcDoc = useMemo(() => {
    // Case-insensitive, first-occurrence injection — the old exact
    // `<head>` match missed `<head id=…>` / `<HEAD>` variants (§P2-24).
    if (/<head[^>]*>/i.test(source)) {
      return source.replace(/<head[^>]*>/i, m => `${m}${STATIC_CSP}`)
    }
    if (/<html[^>]*>/i.test(source)) {
      return source.replace(/<html[^>]*>/i, m => `${m}<head>${STATIC_CSP}</head>`)
    }
    return `<!DOCTYPE html><html><head>${STATIC_CSP}</head><body>${source}</body></html>`
  }, [source])

  // URL of the currently registered artifact, or null while registering /
  // after a failed registration. Loading decision (§5-1 A review note):
  // while registering we render the static srcDoc fallback *immediately* so
  // the panel never flashes empty, then swap to the live URL when it
  // arrives — the static frame is harmless (same content, no scripts).
  const [interactiveUrl, setInteractiveUrl] = useState<string | null>(null)
  const registeredIdRef = useRef<string | null>(null)
  const failedNotifiedRef = useRef(false)

  useEffect(() => {
    if (!interactive) {
      // Prop flipped off while mounted (defensive — RightDock keeps the
      // decision stable per artifact): release any live registration so the
      // registry entry never outlives the interactive rendering.
      const live = registeredIdRef.current
      if (live) {
        registeredIdRef.current = null
        setInteractiveUrl(null)
        unregisterInteractiveArtifact(live).catch(() => {})
      }
      return
    }
    let cancelled = false
    // New source (or first mount): drop back to static until the fresh
    // registration resolves.
    setInteractiveUrl(null)
    failedNotifiedRef.current = false
    registerInteractiveHtml(source)
      .then(reg => {
        if (cancelled) {
          // Unmounted (or the source changed again) before this resolved:
          // drop the freshly registered content right away.
          unregisterInteractiveArtifact(reg.id).catch(() => {})
          return
        }
        const previous = registeredIdRef.current
        registeredIdRef.current = reg.id
        setInteractiveUrl(reg.url)
        if (previous) unregisterInteractiveArtifact(previous).catch(() => {})
      })
      .catch(() => {
        // Registration unavailable (web dev without the command, ACL
        // denial, oversize artifact): stay on the static srcDoc path.
        if (cancelled) return
        registeredIdRef.current = null
        setInteractiveUrl(null)
        if (!failedNotifiedRef.current) {
          failedNotifiedRef.current = true
          onRegistrationFailed?.()
        }
      })
    return () => {
      cancelled = true
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps -- one-shot notify callback; callers pass stable/inline handlers
  }, [interactive, source])

  // Unmount: the registry entry must not outlive the renderer.
  useEffect(
    () => () => {
      const id = registeredIdRef.current
      if (id) unregisterInteractiveArtifact(id).catch(() => {})
    },
    [],
  )

  if (interactive && interactiveUrl) {
    return (
      <iframe
        title={title || 'HTML preview'}
        src={interactiveUrl}
        sandbox={INTERACTIVE_SANDBOX}
        loading="lazy"
        className="w-full h-full bg-white border-0"
        style={{ minHeight: '300px' }}
      />
    )
  }

  return (
    <iframe
      title={title || 'HTML preview'}
      srcDoc={srcDoc}
      // Empty sandbox — strictest form (same posture as MermaidRenderer):
      // opaque origin, and script execution impossible even under a dev
      // CSP that would otherwise allow it.
      sandbox=""
      loading="lazy"
      className="w-full h-full bg-white border-0"
      style={{ minHeight: '300px' }}
    />
  )
}
