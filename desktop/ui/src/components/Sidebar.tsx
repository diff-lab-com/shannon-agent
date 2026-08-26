import { useState, useCallback, useEffect, useRef, memo } from 'react';
import { NavLink, useLocation, useNavigate } from 'react-router-dom';
import { useIntl } from 'react-intl';
import { Button } from '@/components/ui/button';
import { ScrollArea } from '@/components/ui/scroll-area';
import EmptyState from './ui/empty-state';
import { WELCOME_EXAMPLES } from './welcomeExamples';
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
const NAV_OPEN_KEY = 'shannon-nav-open'
export const SIDEBAR_MODE_KEY = 'shannon-sidebar-mode'
export type SidebarMode = 'simple' | 'dev'

// U6: nav IA groups — Work / Resources / Experiments (+ the nested
// Extensions and Settings disclosure). All expansion states persist to
// localStorage under one key so a reload keeps the user's sidebar shape.
type NavGroupId = 'work' | 'resources' | 'experiments' | 'extensions' | 'settings'
type NavOpenMap = Record<NavGroupId, boolean>

function readNavOpen(mode: SidebarMode): NavOpenMap {
  const fallback: NavOpenMap = {
    work: true,
    // Simple mode starts with Resources folded (U6: only Work + Extensions
    // visible); dev mode unfolds it.
    resources: mode === 'dev',
    experiments: true,
    extensions: true,
    settings: false,
  }
  if (typeof window === 'undefined') return fallback
  try {
    const raw = window.localStorage.getItem(NAV_OPEN_KEY)
    return raw ? { ...fallback, ...JSON.parse(raw) } : fallback
  } catch { return fallback }
}

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

// U6: nav group — small uppercase disclosure header + full nav links
// underneath. Expansion state is owned by the caller (persisted).
function NavGroup({ labelId, open, onToggle, children }: {
  labelId: string
  open: boolean
  onToggle: () => void
  children: React.ReactNode
}) {
  const intl = useIntl()
  return (
    <div className="space-y-1">
      <Button
        variant="ghost"
        onClick={onToggle}
        aria-expanded={open}
        className="w-full justify-between px-4 py-1.5 rounded-lg font-label-sm text-[11px] uppercase tracking-wider text-on-surface-variant hover:text-primary transition-all h-auto"
      >
        {intl.formatMessage({ id: labelId })}
        <span className="material-symbols-outlined icon-sm transition-transform duration-200" style={{ transform: open ? 'rotate(180deg)' : 'rotate(0deg)' }} aria-hidden="true">expand_more</span>
      </Button>
      {open && <div className="space-y-1">{children}</div>}
    </div>
  )
}

export const Sidebar = memo(function Sidebar({ mobile }: { mobile?: boolean }) {
  const { close: closeMobile } = useSidebar();
  const [mode, toggleMode] = useSidebarMode();
  const [navOpen, setNavOpen] = useState<NavOpenMap>(() => readNavOpen(mode));
  const toggleNav = useCallback((key: NavGroupId) => {
    setNavOpen(prev => {
      const next = { ...prev, [key]: !prev[key] }
      try { window.localStorage.setItem(NAV_OPEN_KEY, JSON.stringify(next)) } catch { /* noop */ }
      return next
    })
  }, []);
  // Mode switches reset group folding to that mode's defaults — a fresh dev
  // session opens Resources instead of inheriting the simple-mode fold.
  // (Skipped on mount so a stored custom shape survives reloads.)
  const mountedMode = useRef(mode)
  useEffect(() => {
    if (mountedMode.current === mode) return
    mountedMode.current = mode
    const next = readNavOpen(mode)
    setNavOpen(next)
    try { window.localStorage.setItem(NAV_OPEN_KEY, JSON.stringify(next)) } catch { /* noop */ }
  }, [mode]);
  const [width, setWidth] = useState(() => {
    const stored = localStorage.getItem(STORAGE_KEY)
    return stored ? Math.min(MAX_W, Math.max(MIN_W, parseInt(stored, 10) || DEFAULT_W)) : DEFAULT_W
  });
  const dragging = useRef(false);
  const location = useLocation();
  const navigate = useNavigate();
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

  // U7: sidebar starter prompt — creates the first session when none exists,
  // then hands the prompt to the composer via /chat navigation state (the
  // same channel the Editor's "Ask AI" button uses).
  const startWithPrompt = async (prompt: string) => {
    if (!currentSessionId) await createSession()
    navigate('/chat', { state: { prefill: prompt } })
    if (mobile) closeMobile()
  }

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
        {/* U8: brand mark `cognitive` (filled) — a knowledge-graph knot reads as
            "connected intelligence" and nods to Shannon's information theory;
            the old `hub` read as generic networking. Alternates considered:
            blur_on (abstract mesh), neurology (organic nodes). */}
        <div className="w-10 h-10 rounded-xl bg-primary flex items-center justify-center text-on-primary shadow-lg shadow-primary/30">
          <span className="material-symbols-outlined" style={{fontVariationSettings: "'FILL' 1"}}>cognitive</span>
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
          region capped at 60% so a long session list can't push it out.
          U7: zero-session users get a guide card instead of a blank rail.
          It shares example copy with WelcomeState (first two of the same
          list) but stays compact — the canvas welcome card owns the full
          four-card pitch, so the two never duplicate. */}
      <div className="flex-1 min-h-0 mb-lg">
        {sessions.length === 0 ? (
          <EmptyState
            icon="forum"
            title={intl.formatMessage({ id: 'sidebar.sessions.empty.title' })}
            description={intl.formatMessage({ id: 'sidebar.sessions.empty.description' })}
            suggestions={WELCOME_EXAMPLES.slice(0, 2).map(ex => ({
              label: intl.formatMessage({ id: ex.titleKey }),
              icon: ex.icon,
              onClick: () => void startWithPrompt(ex.prompt),
            }))}
          />
        ) : (
          <SessionsSection
            sessions={sessions}
            currentSessionId={currentSessionId}
            switchSession={switchSession}
            renameSession={renameSession}
            deleteSession={deleteSession}
            closeMobile={mobile ? closeMobile : undefined}
          />
        )}
      </div>

      <nav aria-label={intl.formatMessage({ id: 'nav.mainNav.aria' })} className="shrink-0 min-h-0 max-h-[60%]">
        <ScrollArea className="h-full">
        {/* U6: nav grouped by mental model — Work (workflow) / Resources
            (data & extensions) / Experiments (dev-only). */}
        <NavGroup labelId="nav.group.work" open={navOpen.work} onToggle={() => toggleNav('work')}>
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
        </NavGroup>

        {/* Simple mode: flat Extensions entry so普通用户 can reach the
            Extensions Hub without switching to dev mode (dev mode keeps the
            collapsible group below). Links to Featured — the hub's index tab. */}
        {mode === 'simple' && (
          <NavLink to="/extensions/featured" className={getNavClass} onClick={handleNavClick}>
            <span className="material-symbols-outlined">extension</span>
            <span className="flex-1">{intl.formatMessage({ id: 'nav.extensions' })}</span>
          </NavLink>
        )}

        <NavGroup labelId="nav.group.resources" open={navOpen.resources} onToggle={() => toggleNav('resources')}>
          <NavLink to="/memory" className={getNavClass} onClick={handleNavClick}>
             <span className="material-symbols-outlined">psychology</span>
             <span className="flex-1">{intl.formatMessage({ id: 'nav.memory' })}</span>
          </NavLink>

          <NavLink to="/usage" className={getNavClass} onClick={handleNavClick}>
             <span className="material-symbols-outlined">monitoring</span>
             <span className="flex-1">{intl.formatMessage({ id: 'nav.usage' })}</span>
          </NavLink>

          {mode === 'dev' && (
          <div className="space-y-1">
            <Button
              variant="ghost"
              onClick={() => toggleNav('extensions')}
              aria-expanded={navOpen.extensions}
              className={cn("w-full flex items-center justify-between gap-3 px-4 py-3 rounded-xl font-label-md text-label-md transition-all duration-300", isExtensionsActive ? "bg-primary/10 text-primary font-bold shadow-sm" : "text-on-surface-variant hover:bg-surface-container-low hover:text-primary hover:-translate-y-0.5")}
            >
              <div className="flex items-center gap-3">
                <span className="material-symbols-outlined">grid_view</span>
                <span>{intl.formatMessage({ id: 'nav.extensions' })}</span>
              </div>
              <span className="material-symbols-outlined icon-md transition-transform duration-200" style={{ transform: navOpen.extensions ? 'rotate(180deg)' : 'rotate(0deg)' }} aria-hidden="true">expand_more</span>
            </Button>

            {navOpen.extensions && (
              <div className="pl-4 pr-2 space-y-1 mt-1 transition-all" aria-label={intl.formatMessage({ id: 'nav.extensions.section.aria' })}>
                 <SubNavLink to="/extensions/skills" labelId="nav.skills" />
                 <SubNavLink to="/extensions/agents" labelId="nav.myAgents" />
                 <SubNavLink to="/extensions/datasources" labelId="nav.dataSources" />
              </div>
            )}
          </div>
          )}
        </NavGroup>

        {mode === 'dev' && (
        <NavGroup labelId="nav.group.experiments" open={navOpen.experiments} onToggle={() => toggleNav('experiments')}>
          {/* U6: OPC flattened to a direct link — its old disclosure held a
              single sub-link, and a disclosure inside a group is two levels
              of folding for one destination. */}
          <NavLink to="/opc" className={getNavClass} onClick={handleNavClick}>
             <span className="material-symbols-outlined">auto_awesome</span>
             <span className="flex-1 flex items-center gap-2">
               {intl.formatMessage({ id: 'nav.opc' })}
               <span className="text-[9px] bg-primary text-on-primary px-1.5 py-0.5 rounded uppercase font-bold tracking-wider">
                 {intl.formatMessage({ id: 'nav.experiment' })}
               </span>
             </span>
          </NavLink>
        </NavGroup>
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
          onClick={() => toggleNav('settings')}
          aria-expanded={navOpen.settings}
          className={cn("w-full flex items-center justify-between gap-3 px-4 py-3 rounded-xl font-label-md text-label-md transition-all duration-300", isSettingsActive ? "bg-primary/10 text-primary font-bold shadow-sm" : "text-on-surface-variant hover:bg-surface-container-low hover:text-primary hover:-translate-y-0.5")}
        >
          <div className="flex items-center gap-3">
            <span className="material-symbols-outlined" style={{fontVariationSettings: "'FILL' 1"}}>settings</span>
            <span>{intl.formatMessage({ id: 'nav.settings' })}</span>
          </div>
          <span className="material-symbols-outlined icon-md transition-transform duration-200" style={{ transform: navOpen.settings ? 'rotate(180deg)' : 'rotate(0deg)' }} aria-hidden="true">expand_more</span>
        </Button>

        {navOpen.settings && (
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
