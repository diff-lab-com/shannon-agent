import { useState, useCallback, useEffect, useRef, memo } from 'react';
import { NavLink, useLocation } from 'react-router-dom';
import { useIntl } from 'react-intl';
import { Button } from '@/components/ui/button';
import { ScrollArea } from '@/components/ui/scroll-area';
import { cn } from '../lib/utils';
import { useSessions } from '@/context/SessionContext';
import { useCatalog } from '@/context/CatalogContext';
import { SessionsSection } from './SidebarSessions';
import { useSidebar } from './Layout';
import { useTriageStats } from '@/hooks/scheduled-tasks';
import { formatShortcut } from '@/lib/platform';

const MIN_W = 200
const MAX_W = 400
const DEFAULT_W = 280
const STORAGE_KEY = 'shannon-sidebar-width'
export const SIDEBAR_MODE_KEY = 'shannon-sidebar-mode'
export type SidebarMode = 'simple' | 'dev'

export function useSidebarMode(): [SidebarMode, () => void] {
  const [mode, setMode] = useState<SidebarMode>(() => {
    if (typeof window === 'undefined') return 'simple'
    return (window.localStorage.getItem(SIDEBAR_MODE_KEY) as SidebarMode) || 'simple'
  })
  const toggle = useCallback(() => {
    setMode(prev => {
      const next = prev === 'simple' ? 'dev' : 'simple'
      window.localStorage.setItem(SIDEBAR_MODE_KEY, next)
      return next
    })
  }, [])
  return [mode, toggle]
}

const getSubNavClass = ({ isActive }: { isActive: boolean }) =>
  cn(
    "flex items-center px-4 py-2 rounded-lg font-label-md text-[13px] transition-all duration-200",
    isActive
      ? "text-primary font-bold"
      : "text-on-surface-variant hover:text-primary"
  );

// Collapsible sub-navigation link: a leading active/inactive dot + a label.
// Replaces 9 identical render-prop NavLinks (extensions / opc / settings).
function SubNavLink({ to, labelId }: { to: string; labelId: string }) {
  const intl = useIntl()
  return (
    <NavLink to={to} className={getSubNavClass}>
      {({ isActive }) => (
        <>
          <span className={cn("w-1.5 h-1.5 rounded-full mr-3 shrink-0", isActive ? "bg-primary" : "bg-outline-variant")} />
          {intl.formatMessage({ id: labelId })}
        </>
      )}
    </NavLink>
  )
}

export const Sidebar = memo(function Sidebar({ mobile }: { mobile?: boolean }) {
  const { close: closeMobile } = useSidebar();
  const [opcOpen, setOpcOpen] = useState(true);
  const [extensionsOpen, setExtensionsOpen] = useState(true);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [mode, toggleMode] = useSidebarMode();
  const [width, setWidth] = useState(() => {
    const stored = localStorage.getItem(STORAGE_KEY)
    return stored ? Math.min(MAX_W, Math.max(MIN_W, parseInt(stored, 10) || DEFAULT_W)) : DEFAULT_W
  });
  const dragging = useRef(false);
  const location = useLocation();
  const { createSession, sessions, currentSessionId, switchSession, renameSession, deleteSession, createSessionInWorktree } = useSessions();
  const { status } = useCatalog();
  const intl = useIntl();
  const { stats: triageStats, refresh: refreshTriageStats } = useTriageStats();

  // Refresh triage stats every 30s, but only while the window is visible —
  // no point polling a desktop app that's backgrounded. Resumes + immediately
  // refreshes on focus. (Full event-driven refresh would need a backend
  // triage-updated emission; see claudedocs/comprehensive-audit-2026-06-29.md P2-6.)
  useEffect(() => {
    if (typeof document === 'undefined') return;
    let interval: ReturnType<typeof setInterval> | undefined;
    const stop = () => { if (interval) { clearInterval(interval); interval = undefined; } };
    const start = () => { stop(); refreshTriageStats(); interval = setInterval(refreshTriageStats, 30000); };
    const onVisibility = () => { if (document.hidden) { stop(); } else { start(); } };
    start();
    document.addEventListener('visibilitychange', onVisibility);
    return () => { stop(); document.removeEventListener('visibilitychange', onVisibility); };
  }, [refreshTriageStats]);

  const handleMouseDown = useCallback((e: React.MouseEvent) => {
    e.preventDefault()
    dragging.current = true
    document.body.style.cursor = 'col-resize'
    document.body.style.userSelect = 'none'
  }, [])

  // U5: double-click resets to the default width; arrow keys resize by 16px
  // (the handle is a focusable separator so keyboard users can widen the
  // sidebar too — P3-1).
  const resetWidth = useCallback(() => {
    setWidth(DEFAULT_W)
    localStorage.setItem(STORAGE_KEY, String(DEFAULT_W))
    document.documentElement.style.setProperty('--sidebar-w', `${DEFAULT_W}px`)
  }, [])

  const handleResizeKey = useCallback((e: React.KeyboardEvent) => {
    if (e.key !== 'ArrowLeft' && e.key !== 'ArrowRight') return
    e.preventDefault()
    const delta = e.key === 'ArrowLeft' ? -16 : 16
    setWidth(prev => {
      const next = Math.min(MAX_W, Math.max(MIN_W, prev + delta))
      localStorage.setItem(STORAGE_KEY, String(next))
      return next
    })
  }, [])

  useEffect(() => {
    const handleMouseMove = (e: MouseEvent) => {
      if (!dragging.current) return
      const next = Math.min(MAX_W, Math.max(MIN_W, e.clientX))
      setWidth(next)
      document.documentElement.style.setProperty('--sidebar-w', `${next}px`)
    }
    const handleMouseUp = () => {
      if (!dragging.current) return
      dragging.current = false
      document.body.style.cursor = ''
      document.body.style.userSelect = ''
      localStorage.setItem(STORAGE_KEY, String(width))
    }
    window.addEventListener('mousemove', handleMouseMove)
    window.addEventListener('mouseup', handleMouseUp)
    return () => {
      window.removeEventListener('mousemove', handleMouseMove)
      window.removeEventListener('mouseup', handleMouseUp)
    }
  }, [width])

  useEffect(() => {
    document.documentElement.style.setProperty('--sidebar-w', `${width}px`)
  }, [width])

  const isOpcActive = location.pathname.includes('/opc') && !location.pathname.includes('/extensions');
  const isExtensionsActive = location.pathname.includes('/extensions');
  const isSettingsActive = location.pathname.includes('/settings');

  const getNavClass = ({ isActive }: { isActive: boolean }) =>
    cn(
      "flex items-center gap-3 px-4 py-3 rounded-xl font-label-md text-label-md transition-all duration-300",
      isActive
        ? "text-primary bg-primary/10 font-bold shadow-sm"
        : "text-on-surface-variant hover:bg-surface-container-low hover:text-primary hover:-translate-y-0.5"
    );

  const handleNavClick = () => { if (mobile) closeMobile() }

  return (
    <aside data-sidebar className={cn(
      "fixed left-0 top-0 h-full bg-surface-container-lowest/70 backdrop-blur-[20px] border-r border-outline-variant/30 flex flex-col py-lg px-md shadow-[4px_0_24px_-12px_color-mix(in_srgb,var(--color-inverse-surface)_15%,transparent)] transition-transform duration-300",
      mobile ? "z-[70] w-[280px]" : "z-50",
    )} style={mobile ? undefined : { width }}>
      {/* Drag handle — 8px hot zone with a 4px visual bar (U5/P3-1: the old
          4px zone was nearly un-hittable). Focusable separator: ←/→ resize,
          double-click resets to 280px. */}
      <div
        role="separator"
        aria-orientation="vertical"
        tabIndex={0}
        aria-valuenow={width}
        aria-valuemin={MIN_W}
        aria-valuemax={MAX_W}
        className="group absolute right-0 top-0 bottom-0 w-2 cursor-col-resize z-10"
        aria-label={intl.formatMessage({ id: 'nav.resize.aria' })}
        title={intl.formatMessage({ id: 'nav.resize.title' })}
        onMouseDown={handleMouseDown}
        onDoubleClick={resetWidth}
        onKeyDown={handleResizeKey}
      >
        <div className="absolute right-0 top-0 bottom-0 w-1 transition-colors group-hover:bg-primary/30 group-focus-visible:bg-primary/30 group-active:bg-primary/50" />
      </div>
      <div className="flex items-center gap-3 mb-xl px-2">
        <div className="w-10 h-10 rounded-xl bg-primary flex items-center justify-center text-on-primary shadow-lg shadow-primary/30">
          <span className="material-symbols-outlined" style={{fontVariationSettings: "'FILL' 1"}}>hub</span>
        </div>
        <div>
          <h1 className="font-headline-md text-[20px] font-bold text-primary leading-tight">Shannon</h1>
          <p className="font-body-sm text-[12px] text-on-surface-variant leading-none">
            {intl.formatMessage({ id: 'nav.tagline' })}
          </p>
        </div>
      </div>

      <Button
        aria-label={intl.formatMessage({ id: 'nav.newChat.aria' })}
        className="mb-xs w-full py-3 px-4 bg-primary text-on-primary rounded-xl font-bold flex items-center justify-center gap-2 hover:shadow-lg hover:shadow-primary/30 active:scale-95 transition-all"
        onClick={createSession}
      >
        <span className="material-symbols-outlined icon-md">add</span>
        <span>{intl.formatMessage({ id: 'nav.newChat' })}</span>
      </Button>
      {mode === 'dev' && (
      <Button
        variant="ghost"
        aria-label={intl.formatMessage({ id: 'sidebar.worktree.new.aria' })}
        title={intl.formatMessage({ id: 'sidebar.worktree.new.title' })}
        className="mb-lg w-full py-2 px-3 text-on-surface-variant hover:text-primary rounded-lg font-label-md text-label-md flex items-center justify-center gap-1.5 hover:bg-surface-container-low transition-all"
        onClick={createSessionInWorktree}
      >
        <span className="material-symbols-outlined icon-sm">account_tree</span>
        <span>{intl.formatMessage({ id: 'sidebar.worktree.new' })}</span>
      </Button>
      )}

      {/* U1: the session rail is the app's only session list. It takes the
          remaining vertical space (own scroll); nav below gets its own scroll
          region capped at 60% so a long session list can't push it out. */}
      {sessions.length > 0 && (
        <div className="flex-1 min-h-0 mb-lg">
          <SessionsSection
            sessions={sessions}
            currentSessionId={currentSessionId}
            switchSession={switchSession}
            renameSession={renameSession}
            deleteSession={deleteSession}
            closeMobile={mobile ? closeMobile : undefined}
          />
        </div>
      )}

      <nav aria-label={intl.formatMessage({ id: 'nav.mainNav.aria' })} className="shrink-0 min-h-0 max-h-[60%]">
        <ScrollArea className="h-full">
        <NavLink to="/chat" className={getNavClass} onClick={handleNavClick}>
           <span className="material-symbols-outlined">chat_bubble</span>
           <span className="flex-1">{intl.formatMessage({ id: 'nav.chat' })}</span>
           <kbd className="text-[10px] px-1.5 py-0.5 rounded bg-surface-container-high text-on-surface font-mono">{formatShortcut('1')}</kbd>
        </NavLink>
        <NavLink to="/tasks" className={getNavClass} onClick={handleNavClick}>
           <span className="material-symbols-outlined">task_alt</span>
           <span className="flex-1">{intl.formatMessage({ id: 'nav.scheduled' })}</span>
           <kbd className="text-[10px] px-1.5 py-0.5 rounded bg-surface-container-high text-on-surface font-mono">{formatShortcut('2')}</kbd>
        </NavLink>
        <NavLink to="/memory" className={getNavClass} onClick={handleNavClick}>
           <span className="material-symbols-outlined">psychology</span>
           <span className="flex-1">{intl.formatMessage({ id: 'nav.memory' })}</span>
        </NavLink>

        <NavLink to="/usage" className={getNavClass} onClick={handleNavClick}>
           <span className="material-symbols-outlined">monitoring</span>
           <span className="flex-1">{intl.formatMessage({ id: 'nav.usage' })}</span>
        </NavLink>

        {/* Triage full-page navigation */}
        <NavLink
          to="/triage"
          aria-label={intl.formatMessage({ id: 'nav.triage.aria' })}
          className={getNavClass}
          onClick={handleNavClick}
        >
          <span className="material-symbols-outlined">inbox</span>
          <span className="flex-1">{intl.formatMessage({ id: 'nav.triage' })}</span>
          {triageStats.unread > 0 && (
            <span className="bg-error text-on-error text-[11px] font-bold px-1.5 py-0.5 rounded-full">
              {triageStats.unread}
            </span>
          )}
        </NavLink>

        {/* Simple mode: flat Extensions entry so普通用户 can reach the
            Extensions Hub without switching to dev mode (dev mode keeps the
            collapsible group below). Links to Featured — the hub's index tab. */}
        {mode === 'simple' && (
          <NavLink to="/extensions/featured" className={getNavClass} onClick={handleNavClick}>
            <span className="material-symbols-outlined">extension</span>
            <span className="flex-1">{intl.formatMessage({ id: 'nav.extensions' })}</span>
          </NavLink>
        )}

        {mode === 'dev' && (
        <>
        <div className="space-y-1">
          <Button
            variant="ghost"
            onClick={() => setExtensionsOpen(!extensionsOpen)}
            className={cn("w-full flex items-center justify-between gap-3 px-4 py-3 rounded-xl font-label-md text-label-md transition-all duration-300", isExtensionsActive ? "bg-primary/10 text-primary font-bold shadow-sm" : "text-on-surface-variant hover:bg-surface-container-low hover:text-primary hover:-translate-y-0.5")}
          >
            <div className="flex items-center gap-3">
              <span className="material-symbols-outlined">grid_view</span>
              <span>{intl.formatMessage({ id: 'nav.extensions' })}</span>
            </div>
            <span className="material-symbols-outlined icon-md transition-transform duration-200" style={{ transform: extensionsOpen ? 'rotate(180deg)' : 'rotate(0deg)' }} aria-hidden="true">expand_more</span>
          </Button>

          {extensionsOpen && (
            <div className="pl-4 pr-2 space-y-1 mt-1 transition-all" aria-label={intl.formatMessage({ id: 'nav.extensions.section.aria' })}>
               <SubNavLink to="/extensions/skills" labelId="nav.skills" />
               <SubNavLink to="/extensions/agents" labelId="nav.myAgents" />
               <SubNavLink to="/extensions/datasources" labelId="nav.dataSources" />
            </div>
          )}
        </div>

        <div className="space-y-1">
          <Button
            variant="ghost"
            onClick={() => setOpcOpen(!opcOpen)}
            className={cn("w-full flex items-center justify-between gap-3 px-4 py-3 rounded-lg font-label-md text-label-md transition-all duration-200", isOpcActive ? "bg-primary/10 text-primary font-bold" : "text-on-surface-variant hover:bg-surface-container-high/50 hover:text-primary")}
          >
            <div className="flex items-center gap-3">
              <span>{intl.formatMessage({ id: 'nav.opc' })}</span>
              <span className="text-[9px] bg-primary text-on-primary px-1.5 py-0.5 rounded uppercase font-bold tracking-wider">
                {intl.formatMessage({ id: 'nav.experiment' })}
              </span>
            </div>
            <span className="material-symbols-outlined icon-md transition-transform duration-200" style={{ transform: opcOpen ? 'rotate(180deg)' : 'rotate(0deg)' }} aria-hidden="true">expand_more</span>
          </Button>

          {opcOpen && (
            <div className="pl-4 pr-2 space-y-1 mt-1 transition-all">
               <SubNavLink to="/opc" labelId="nav.onePersonCompany" />
            </div>
          )}
        </div>

        </>
        )}
        </ScrollArea>
      </nav>

      <div className="mt-auto pt-lg border-t border-outline-variant/20 space-y-1">
        <Button
          variant="ghost"
          onClick={toggleMode}
          className="w-full justify-between gap-3 px-4 py-2 rounded-lg font-label-md text-[12px] text-on-surface-variant hover:bg-surface-container-low hover:text-primary cursor-pointer transition-all h-auto"
          aria-label={intl.formatMessage({ id: mode === 'simple' ? 'nav.simpleMode.aria' : 'nav.devMode.aria' })}
          aria-pressed={mode === 'dev'}
          title={intl.formatMessage({ id: mode === 'simple' ? 'nav.simpleMode.title' : 'nav.devMode.title' })}
        >
          <div className="flex items-center gap-2">
            <span className="material-symbols-outlined text-[18px]">{mode === 'simple' ? 'tune' : 'dashboard_customize'}</span>
            <span>
              {intl.formatMessage({ id: mode === 'simple' ? 'nav.modeLabel.simple' : 'nav.modeLabel.dev' })}
            </span>
          </div>
          <span className="text-[10px] uppercase tracking-wider text-on-surface-variant">
            {intl.formatMessage({ id: mode === 'simple' ? 'nav.simpleMode.badge' : 'nav.devMode.badge' })}
          </span>
        </Button>
        <Button
          variant="ghost"
          onClick={() => setSettingsOpen(!settingsOpen)}
          className={cn("w-full flex items-center justify-between gap-3 px-4 py-3 rounded-xl font-label-md text-label-md transition-all duration-300", isSettingsActive ? "bg-primary/10 text-primary font-bold shadow-sm" : "text-on-surface-variant hover:bg-surface-container-low hover:text-primary hover:-translate-y-0.5")}
        >
          <div className="flex items-center gap-3">
            <span className="material-symbols-outlined" style={{fontVariationSettings: "'FILL' 1"}}>settings</span>
            <span>{intl.formatMessage({ id: 'nav.settings' })}</span>
          </div>
          <span className="material-symbols-outlined icon-md transition-transform duration-200" style={{ transform: settingsOpen ? 'rotate(180deg)' : 'rotate(0deg)' }} aria-hidden="true">expand_more</span>
        </Button>

        {settingsOpen && (
          <div className="pl-4 pr-2 space-y-1 mt-1 transition-all" aria-label={intl.formatMessage({ id: 'nav.settings.section.aria' })}>
             <SubNavLink to="/settings/general" labelId="nav.general" />
             <SubNavLink to="/settings/theme" labelId="nav.theme" />
             <SubNavLink to="/settings/models" labelId="nav.models" />
             {mode === 'dev' && (
               <>
                 <SubNavLink to="/settings/billing" labelId="nav.usageBilling" />
                 <SubNavLink to="/settings/advanced" labelId="nav.advanced" />
               </>
             )}
             <SubNavLink to="/settings/notifications" labelId="nav.notifications" />
             <SubNavLink to="/settings/connections" labelId="nav.connections" />
          </div>
        )}

        {/* Status bar */}
        {status && (
          <div className="mt-sm px-2 py-sm flex items-center gap-sm text-label-sm text-on-surface-variant">
            <span className="w-2 h-2 rounded-full bg-tertiary shrink-0"></span>
            <span className="truncate">{status.model}</span>
          </div>
        )}
      </div>
    </aside>
  );
});
