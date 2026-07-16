import { useState, useCallback, useEffect, createContext, useContext } from 'react';
import { Outlet, useNavigate } from 'react-router-dom';
import { useIntl } from 'react-intl';
import { Sidebar } from './Sidebar';
import { Header } from './Header';
import { ErrorBoundary } from '@/components/ErrorBoundary'
import CommandPalette from './CommandPalette';
import KeyboardShortcutsHelp from './KeyboardShortcutsHelp';
import { useChat } from '@/context/ChatContext';
import { useSessions } from '@/context/SessionContext';
import { useCatalog } from '@/context/CatalogContext';
import { useKeyboardShortcuts } from '@/hooks/useKeyboardShortcuts';
import { shouldShowWelcome } from '@/pages/Welcome';

interface SidebarContextValue {
  open: boolean
  toggle: () => void
  close: () => void
}

const SidebarContext = createContext<SidebarContextValue>({ open: false, toggle: () => {}, close: () => {} })
export const useSidebar = () => useContext(SidebarContext)

export function Layout() {
  const { usage } = useChat();
  const { sessions, createSession } = useSessions();
  const { agents, status, backgroundTasks, config, loading } = useCatalog();
  const navigate = useNavigate();
  const intl = useIntl();
  const [paletteOpen, setPaletteOpen] = useState(false);
  const [helpOpen, setHelpOpen] = useState(false);
  const [sidebarOpen, setSidebarOpen] = useState(false);
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

  useEffect(() => {
    if (shouldShowWelcome(loading, !!config?.provider)) {
      navigate('/welcome', { replace: true })
    }
  }, [loading, config, navigate])

  const activeBgTasks = backgroundTasks.filter(t => t.status === 'running').length
  const version = config?.version ?? ''

  return (
    <SidebarContext.Provider value={{ open: sidebarOpen, toggle: toggleSidebar, close: closeSidebar }}>
      <div className="bg-background text-on-surface font-body-md overflow-hidden min-h-screen">
        {/* Mobile sidebar overlay */}
        {sidebarOpen && (
          <div className="fixed inset-0 z-[60] bg-black/40 backdrop-blur-sm md:hidden" onClick={closeSidebar} />
        )}
        <div className="md:hidden">
          <Sidebar mobile />
        </div>
        <div className="hidden md:block">
          <Sidebar />
        </div>
        <Header />
        <CommandPalette open={paletteOpen} onClose={() => setPaletteOpen(false)} />
        <KeyboardShortcutsHelp open={helpOpen} onClose={() => setHelpOpen(false)} />
        <main role="main" className="pt-16 pb-8 h-screen flex flex-col relative" style={{ marginLeft: 'var(--sidebar-w)', width: 'calc(100% - var(--sidebar-w))' }}>
          <ErrorBoundary><Outlet /></ErrorBoundary>
        </main>
        <footer role="contentinfo" className="fixed bottom-0 right-0 h-8 bg-surface-container-low/90 backdrop-blur-sm border-t border-outline-variant/20 flex items-center justify-between px-lg z-40" style={{ left: 'var(--sidebar-w)' }}>
          <span className="font-label-sm text-label-sm text-on-surface-variant flex items-center gap-sm">
            {usage ? (
              <>
                <span className="w-2 h-2 rounded-full bg-tertiary shrink-0" />
                <span>{intl.formatMessage({ id: 'footer.tokens' }, { count: (usage.input_tokens + usage.output_tokens) })}</span>
                <span className="text-outline-variant">·</span>
                <span className="text-primary">${usage.cost_usd.toFixed(4)}</span>
              </>
            ) : status ? (
              <>
                <span className={`w-2 h-2 rounded-full shrink-0 ${status.querying ? 'bg-secondary animate-pulse' : 'bg-tertiary'}`} />
                <span>{status.provider}</span>
                <span className="text-outline-variant">·</span>
                <span className="truncate max-w-[140px]">{status.model}</span>
              </>
            ) : (
              <>
                <span className="w-2 h-2 rounded-full bg-outline shrink-0" />
                <span>{intl.formatMessage({ id: 'app.brandName' })}</span>
              </>
            )}
          </span>
          <div className="flex items-center gap-md font-label-sm text-label-sm text-on-surface-variant">
            {sessions.length > 0 && (
              <span className="hidden sm:inline">
                {intl.formatMessage({ id: 'footer.sessions' }, { count: sessions.length })}
              </span>
            )}
            {activeBgTasks > 0 && (
              <span className="flex items-center gap-xs text-primary">
                <span className="w-2 h-2 rounded-full bg-secondary animate-pulse" />
                {intl.formatMessage({ id: 'footer.tasks' }, { count: activeBgTasks })}
              </span>
            )}
            {agents.length > 0 && (
              <span className="hidden sm:flex items-center gap-xs text-primary">
                <span className="w-2 h-2 rounded-full bg-secondary animate-pulse" />
                {intl.formatMessage({ id: 'footer.agents' }, { count: agents.length })}
              </span>
            )}
            {version && (
              <span className="hidden md:inline text-outline-variant">v{version}</span>
            )}
          </div>
        </footer>
      </div>
    </SidebarContext.Provider>
  );
}
