import { useState, useCallback, useEffect, useRef, createContext, useContext, Suspense } from 'react';
import { Outlet, useLocation, useNavigate } from 'react-router-dom';
import { useIntl } from 'react-intl';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { Sidebar, readStoredSidebarWidth } from './Sidebar';
import { Header } from './Header';
import { ErrorBoundary } from '@/components/ErrorBoundary'
import { Banner } from '@/components/ui/banner';
import { Button } from '@/components/ui/button';
import CommandPalette from './CommandPalette';
import KeyboardShortcutsHelp from './KeyboardShortcutsHelp';
import { useSessions } from '@/context/SessionContext';
import { useCatalog } from '@/context/CatalogContext';
import { useKeyboardShortcuts } from '@/hooks/useKeyboardShortcuts';
import { useDocumentTitle } from '@/hooks/useDocumentTitle';
import { shouldShowWelcome } from '@/pages/Welcome';
import { listen } from '@tauri-apps/api/event';
import { SESSION_WINDOW_REVEAL_EVENT, parseWindowSession } from '@/lib/windowSession';
import { dispatchPaletteVisibility } from '@/lib/paletteVisibility';

// H3 (导航批, plan §2.3-4): restart-restore of the last visited route —
// the Slack/Discord/Obsidian implicit contract. The main window records
// its settled location under this key (debounced); on the next boot the
// stored route replaces the default /chat landing. Session windows never
// participate (they are pinned to one session and share this localStorage).
const LAST_ROUTE_KEY = 'shannon.lastRoute'
const LAST_ROUTE_WRITE_DEBOUNCE_MS = 300

/**
 * Routes that must never be recorded — and never restored into (defensive
 * on read, so a stale pre-exclusion value can't resurrect them either):
 *  - `/` — the transient RootRedirect hop, never a real destination
 *  - `/welcome` — first-run onboarding; must not shadow the default landing
 *  - `/timeline/:id` — in-turn deep page tied to a session that is gone
 *    after restart
 *  - `/companion` — the Quick Capture window's chrome-less page; a main
 *    window only ever shows it as a manual fallback
 */
function isTransientRoute(pathname: string): boolean {
  return pathname === '/'
    || pathname === '/welcome'
    || pathname.startsWith('/timeline/')
    || pathname === '/companion'
}

interface SidebarContextValue {
  open: boolean
  toggle: () => void
  close: () => void
  /** B1-10: the Sidebar reports its width here; Layout is the single
      writer of the `--sidebar-w` CSS variable. */
  reportWidth: (width: number) => void
}

const SidebarContext = createContext<SidebarContextValue>({ open: false, toggle: () => {}, close: () => {}, reportWidth: () => {} })
export const useSidebar = () => useContext(SidebarContext)

/** B1-16: chunk-loading fallback for lazy routes. Lives at the Outlet (not
 *  the app root) so the shell — sidebar, header — stays mounted
 *  while a page chunk loads instead of the whole skeleton flashing away. */
export function PageLoader() {
  return (
    <div className="flex-1 flex items-center justify-center">
      <span className="material-symbols-outlined icon-xl text-primary animate-spin">progress_activity</span>
    </div>
  )
}

export function Layout() {
  const { createSession, sessions, switchSession, windowSessionId } = useSessions();
  const { providerStatus, loading, initError, retryInit } = useCatalog();
  const navigate = useNavigate();
  // B1-12 (review P1-7): remounts the route ErrorBoundary on navigation so a
  // crashed page's fallback can never outlive its route — without the key,
  // one crash covered every page visited afterwards.
  const location = useLocation();
  const intl = useIntl();
  // P1-1 window mode: this window is pinned to one session — sidebar hidden
  // (lowest-cost slim chrome; nav lives in the main window), content spans
  // the full width, and the native window title tracks the session title.
  const isWindowMode = windowSessionId != null;
  const [paletteOpen, setPaletteOpen] = useState(false);
  const [helpOpen, setHelpOpen] = useState(false);
  const [sidebarOpen, setSidebarOpen] = useState(false);
  // Single Sidebar instance; the drawer mode is chosen at runtime via the
  // media-query state below. Earlier code rendered two full trees, and the
  // duplicate was responsible for a cascade of CI flakes (Playwright strict-
  // mode duplicate hits, hit-test shadow on the mobile copy).
  // Why a mobile branch at all: Tauri minWidth=800 (desktop/tauri.conf.json)
  // keeps the desktop window above the 768px breakpoint, so ≤767px is
  // unreachable there — this drawer form is retained only for
  // e2e/mobile-drawer.spec.ts (pins a 375×812 viewport and asserts the
  // drawer/scrim contract) and pure-browser `pnpm dev`. Don't delete it
  // without migrating that spec first.
  const [mobileMode, setMobileMode] = useState(false);
  useEffect(() => {
    const mq = window.matchMedia('(max-width: 767px)')
    const update = () => setMobileMode(mq.matches)
    update()
    mq.addEventListener('change', update)
    return () => mq.removeEventListener('change', update)
  }, [])
  // B1-10 (review P1-4 / R1-2): Layout is the single writer of the
  // `--sidebar-w` CSS variable. The Sidebar only reports its width through
  // the context below. This closes the review's hole — the old split
  // (Layout wrote 0px for window/mobile, the Sidebar wrote the desktop
  // width) left the variable stuck at 280px after a desktop→mobile→desktop
  // round-trip, because the Sidebar's own effect keyed on `[width]` never
  // re-fired. The desktop branch now writes the reported width on EVERY
  // mobileMode toggle.
  const [sidebarWidth, setSidebarWidth] = useState(readStoredSidebarWidth);
  const togglePalette = useCallback(() => setPaletteOpen(p => !p), []);
  const toggleHelp = useCallback(() => setHelpOpen(p => !p), []);
  const toggleSidebar = useCallback(() => setSidebarOpen(p => !p), []);
  const closeSidebar = useCallback(() => setSidebarOpen(false), []);
  const handleNewSession = useCallback(() => { void createSession() }, [createSession]);
  useKeyboardShortcuts(togglePalette, toggleHelp, handleNewSession);

  // H3 (导航批, plan §2.3-3): window document.title follows the route —
  // same page-context string the Header renders (lib/pageTitle + U2 chat
  // session title), suffixed with the app.brandName brand. Mounted here
  // once, at the same layer as the Header, never per page.
  useDocumentTitle();

  // H3 (导航批, plan §2.3-4): record the settled route for next-launch
  // restore. Debounced so a rapid nav chain (boot redirects, palette hops)
  // only lands the LAST location; transient routes and session windows are
  // skipped (see isTransientRoute — localStorage is shared across windows,
  // and a pinned session window must never overwrite the main window's slot).
  useEffect(() => {
    if (isWindowMode) return
    if (parseWindowSession(location.search)) return
    if (isTransientRoute(location.pathname)) return
    const { pathname, search } = location
    const timer = window.setTimeout(() => {
      try {
        window.localStorage.setItem(LAST_ROUTE_KEY, pathname + search)
      } catch { /* storage unavailable (private mode) — restore is best-effort */ }
    }, LAST_ROUTE_WRITE_DEBOUNCE_MS)
    return () => window.clearTimeout(timer)
  }, [location, isWindowMode])

  // H3 (导航批, plan §2.3-4): boot-time restore. Runs AFTER the declarative
  // redirect chain (RootRedirect / legacy redirects / wildcard) — those are
  // child-route <Navigate>s that commit before this parent effect re-fires,
  // and the restore is keyed to the DEFAULT LANDING only: an explicit deep
  // link (/settings, /quickfix → /tasks, anything else) always wins.
  // Priority vs. existing startup behavior: first-run onboarding still wins
  // — the shouldShowWelcome effect below replaces this navigation when the
  // provider setup hasn't been completed.
  const restoreDoneRef = useRef(false);
  useEffect(() => {
    if (restoreDoneRef.current) return
    // Session windows (/?windowSession=<uuid>) are pinned to one session —
    // never restore a main-window route into them.
    if (isWindowMode || parseWindowSession(location.search)) {
      restoreDoneRef.current = true
      return
    }
    // `/` = RootRedirect hasn't committed yet — wait for it to land on
    // /chat before deciding. Any other non-default path is a deliberate
    // deep link: mark attempted and stand down.
    if (location.pathname !== '/' && location.pathname !== '/chat') {
      restoreDoneRef.current = true
      return
    }
    if (location.pathname === '/') return
    restoreDoneRef.current = true
    let stored: string | null = null
    try {
      stored = window.localStorage.getItem(LAST_ROUTE_KEY)
    } catch { /* ignore — restore is best-effort */ }
    if (!stored || !stored.startsWith('/')) return
    // Defensive: never restore INTO a transient route (a value written
    // before the exclusion list shipped).
    const queryIndex = stored.indexOf('?')
    const storedPath = queryIndex === -1 ? stored : stored.slice(0, queryIndex)
    if (isTransientRoute(storedPath)) return
    // replace:true — the stored route takes over the boot entry, so the
    // back arrow doesn't offer a return to the bare /chat hop.
    navigate(stored, { replace: true })
  }, [location.pathname, location.search, isWindowMode, navigate])

  useEffect(() => {
    const handler = () => setHelpOpen(p => !p)
    window.addEventListener('shannon:toggle-help', handler)
    return () => window.removeEventListener('shannon:toggle-help', handler)
  }, [])

  // Batch B4: the sidebar's 搜索 action opens the palette through the same
  // shannon:* window-event convention as toggle-help / open-editor.
  useEffect(() => {
    const handler = () => setPaletteOpen(p => !p)
    window.addEventListener('shannon:toggle-palette', handler)
    return () => window.removeEventListener('shannon:toggle-palette', handler)
  }, [])

  // Aurora redesign F-9 companion: while the palette is open, the chat
  // page's RightDock drops its backdrop-filter (`.dock-solid`) so a fresh
  // glass layer never stacks on the dock's (on-screen budget ≤ 4 holds).
  // Layout owns the palette state; the dock lives in route pages, so the
  // state crosses via the shannon:* window-event convention (see
  // lib/paletteVisibility). Fires on mount too, so late subscribers
  // (navigating into /chat with the palette already up) start correct.
  useEffect(() => {
    dispatchPaletteVisibility(paletteOpen)
  }, [paletteOpen])

  // 2026-09-29 provider review §3-A1: `config.provider` is dead since
  // ADR-0005 (always undefined) — the gate ran on a permanent "no
  // provider". Use the reliable snapshot; an env-detected provider
  // (ANTHROPIC_API_KEY etc.) counts as configured, same as the backend.
  useEffect(() => {
    const hasProvider = !!providerStatus
      && (providerStatus.active_provider_id != null || providerStatus.env_provider != null)
    if (shouldShowWelcome(loading, hasProvider)) {
      navigate('/welcome', { replace: true })
    }
  }, [loading, providerStatus, navigate])

  // B1-10: single `--sidebar-w` write point — 0px while the sidebar is a
  // drawer (mobile) or absent (window mode), the Sidebar-reported width on
  // desktop.
  useEffect(() => {
    if (isWindowMode || mobileMode) {
      document.documentElement.style.setProperty('--sidebar-w', '0px')
    } else {
      document.documentElement.style.setProperty('--sidebar-w', `${sidebarWidth}px`)
    }
  }, [isWindowMode, mobileMode, sidebarWidth])

  // P1-1 window mode: keep the native window title in sync with the session
  // title (follows renames and Tier-1 auto-titling via the sessions list).
  useEffect(() => {
    if (!isWindowMode) return
    const title = sessions.find(s => s.id === windowSessionId)?.title
    if (!title) return
    getCurrentWindow().setTitle(title).catch(() => { /* best-effort chrome sync */ })
  }, [isWindowMode, sessions, windowSessionId])

  // P1-1: a session window's「在主窗口打开」focused us and asked for a
  // session switch (backend emits this only at `main`). Requires the
  // `core:event` capability granted by capabilities/session-windows.json.
  useEffect(() => {
    let cancelled = false
    let unlisten: (() => void) | undefined
    void listen(SESSION_WINDOW_REVEAL_EVENT, (e) => {
      const sessionId = (e.payload as { sessionId?: string }).sessionId
      if (!sessionId) return
      void switchSession(sessionId).then(() => navigate('/chat'))
    }).then(fn => {
      if (cancelled) { fn(); return }
      unlisten = fn
    })
    return () => {
      cancelled = true
      unlisten?.()
    }
  }, [switchSession, navigate])

  // 裁决 D (2026-10-15 parity review §5): the Layout footer is GONE. The
  // design has exactly one status line — the chat page's ChatStatusBar
  // (02c D5) — and the footer's brand/tasks/version/tokens segments were the
  // "double status bar" the review called out. There is no exception state
  // left to render here: backend init failure already gets the error Banner
  // above, and the demo identity lives on the pages that are demos. The
  // footer.* i18n keys are retired with it.

  return (
    <SidebarContext.Provider value={{ open: sidebarOpen, toggle: toggleSidebar, close: closeSidebar, reportWidth: setSidebarWidth }}>
      {/* G2: no bg here on purpose — the root div is transparent so the
          body's --material-base (L0 window base) shows around the sidebar
          rail, while <main> paints --color-surface for the content tier. */}
      <div className="text-on-surface font-body-md overflow-hidden min-h-screen">
        {/* Mobile sidebar overlay — scrim (遮罩), not a glass material: the
            direct backdrop-blur here is intentional and guard-exempt.
            Reachable only when the ≤767px media query above matches (Tauri
            minWidth=800 never gets there) — kept for
            e2e/mobile-drawer.spec.ts, which asserts this scrim's open/close. */}
        {sidebarOpen && (
          <div className="fixed inset-0 z-scrim bg-black/40 backdrop-blur-sm md:hidden" onClick={closeSidebar} />
        )}
        {/* P1-1 window mode: no sidebar rail — the window is pinned to one
            session and the Header carries the window controls. Single
            Sidebar instance; the drawer mode is chosen at runtime via the
            media-query state below. Earlier code rendered two full trees,
            and the duplicate was responsible for a cascade of CI flakes
            (Playwright strict-mode duplicate hits, hit-test shadow on the
            mobile copy). */}
        {!isWindowMode && <Sidebar mobile={mobileMode} open={mobileMode ? sidebarOpen : true} />}
        <Header />
        <CommandPalette open={paletteOpen} onClose={() => setPaletteOpen(false)} />
        <KeyboardShortcutsHelp open={helpOpen} onClose={() => setHelpOpen(false)} />
        {/* G2: the content tier paints surface over the body's L0 base so
            cards (container-lowest) keep their layer against it.
            aurora-ambient (W1 视觉收敛): the design's fixed `.ambient`
            aurora wash rides on main's own background — viewport-anchored
            fixed attachment, above the surface fill, under every route
            content; plain gradients, no backdrop-filter (blur budget ≤4).
            Theme intensity is handled inside index.css (data-theme-mode). */}
        {/* 裁决 D: footer removed entirely (整条隐藏) — the content tier now
            owns the full window height; see the comment above the return. */}
        <main role="main" className="pt-16 h-screen flex flex-col relative bg-surface aurora-ambient" style={{ marginLeft: 'var(--sidebar-w)', width: 'calc(100% - var(--sidebar-w))' }}>
          {initError && (
            <Banner tone="error" className="items-center shrink-0">
              <span className="material-symbols-outlined icon-md text-error shrink-0" aria-hidden="true">error</span>
              <p className="flex-1 min-w-0 font-label-md text-on-surface">
                {intl.formatMessage({ id: 'app.init.failed' })}
              </p>
              <Button variant="outline" size="sm" className="shrink-0" onClick={() => void retryInit()}>
                {intl.formatMessage({ id: 'app.init.retry' })}
              </Button>
            </Banner>
          )}
          {/* B1-16: Suspense at the Outlet level — lazy page chunks load
              inside the shell, so only the content area shows the loader. */}
          <ErrorBoundary key={location.pathname}>
            <Suspense fallback={<PageLoader />}>
              <Outlet />
            </Suspense>
          </ErrorBoundary>
        </main>
      </div>
    </SidebarContext.Provider>
  );
}
