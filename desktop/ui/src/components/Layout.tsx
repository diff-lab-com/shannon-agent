import { useState, useCallback, useEffect, createContext, useContext, Suspense } from 'react';
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
import { useChat } from '@/context/ChatContext';
import { useSessions } from '@/context/SessionContext';
import { useCatalog } from '@/context/CatalogContext';
import { useKeyboardShortcuts } from '@/hooks/useKeyboardShortcuts';
import { shouldShowWelcome } from '@/pages/Welcome';
import { listen } from '@tauri-apps/api/event';
import { SESSION_WINDOW_REVEAL_EVENT } from '@/lib/windowSession';

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
 *  the app root) so the shell — sidebar, header, footer — stays mounted
 *  while a page chunk loads instead of the whole skeleton flashing away. */
export function PageLoader() {
  return (
    <div className="flex-1 flex items-center justify-center">
      <span className="material-symbols-outlined icon-xl text-primary animate-spin">progress_activity</span>
    </div>
  )
}

export function Layout() {
  const { usage } = useChat();
  const { createSession, sessions, switchSession, windowSessionId } = useSessions();
  const { backgroundTasks, config, loading, initError, retryInit } = useCatalog();
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

  useEffect(() => {
    if (shouldShowWelcome(loading, !!config?.provider)) {
      navigate('/welcome', { replace: true })
    }
  }, [loading, config, navigate])

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

  const activeBgTasks = backgroundTasks.filter(t => t.status === 'running').length
  const version = config?.version ?? ''

  return (
    <SidebarContext.Provider value={{ open: sidebarOpen, toggle: toggleSidebar, close: closeSidebar, reportWidth: setSidebarWidth }}>
      <div className="bg-background text-on-surface font-body-md overflow-hidden min-h-screen">
        {/* Mobile sidebar overlay */}
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
        <main role="main" className="pt-16 pb-footer h-screen flex flex-col relative" style={{ marginLeft: 'var(--sidebar-w)', width: 'calc(100% - var(--sidebar-w))' }}>
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
        <footer role="contentinfo" className="fixed bottom-0 right-0 h-footer bg-surface-container-low/90 backdrop-blur-sm border-t border-outline-variant/20 flex items-center justify-between px-lg z-header" style={{ left: 'var(--sidebar-w)' }}>
          {/* U2: footer carries runtime + usage only — tokens/cost, active
              tasks, version. Provider/model live in the Header and the
              session count is visible in the sidebar rail (U1). U9: the
              agents count was removed — it counted agent *definitions*, not
              running work, so it had no action meaning for the user. */}
          <span className="font-label-sm text-label-sm text-on-surface-variant flex items-center gap-sm">
            {usage ? (
              <>
                <span className="w-2 h-2 rounded-full bg-tertiary shrink-0" />
                <span>{intl.formatMessage({ id: 'footer.tokens' }, { count: (usage.input_tokens + usage.output_tokens) })}</span>
                <span className="text-outline-variant">·</span>
                <span className="text-primary">${usage.cost_usd.toFixed(4)}</span>
              </>
            ) : (
              <>
                <span className="w-2 h-2 rounded-full bg-outline shrink-0" />
                <span>{intl.formatMessage({ id: 'app.brandName' })}</span>
              </>
            )}
          </span>
          <div className="flex items-center gap-md font-label-sm text-label-sm text-on-surface-variant">
            {activeBgTasks > 0 && (
              <span className="flex items-center gap-xs text-on-surface-variant">
                <span className="w-2 h-2 rounded-full bg-secondary animate-pulse" />
                {intl.formatMessage({ id: 'footer.tasks' }, { count: activeBgTasks })}
              </span>
            )}
            {version && (
              <span className="hidden md:inline text-on-surface-variant">v{version}</span>
            )}
          </div>
        </footer>
      </div>
    </SidebarContext.Provider>
  );
}
