import { useState, useCallback, useEffect, useRef, useMemo, memo } from 'react';
import { NavLink, useLocation } from 'react-router-dom';
import { useIntl } from 'react-intl';
import { Button } from '@/components/ui/button';
import { ScrollArea } from '@/components/ui/scroll-area';
import { cn } from '../lib/utils';
import { useApp } from '@/context/AppContext';
import type { SessionInfo } from '@/types';
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

interface SessionsSectionProps {
  sessions: SessionInfo[]
  currentSessionId: string | null
  switchSession: (id: string) => Promise<void>
  closeMobile?: () => void
}

const SESSIONS_ORDER_KEY = 'shannon-sessions-order'
const SESSIONS_LIMIT = 8

function SessionsSection({ sessions, currentSessionId, switchSession, closeMobile }: SessionsSectionProps) {
  const intl = useIntl()
  const [query, setQuery] = useState('')
  const [draggedId, setDraggedId] = useState<string | null>(null)
  const [orderOverride, setOrderOverride] = useState<Record<string, number>>(() => {
    if (typeof window === 'undefined') return {}
    try {
      const raw = window.localStorage.getItem(SESSIONS_ORDER_KEY)
      return raw ? JSON.parse(raw) : {}
    } catch { return {} }
  })

  // Sort: explicit order override first (ascending), then by created_at desc
  const sorted = useMemo(() => {
    const withOrder = sessions.filter(s => orderOverride[s.id] !== undefined)
      .sort((a, b) => orderOverride[a.id] - orderOverride[b.id])
    const withoutOrder = sessions.filter(s => orderOverride[s.id] === undefined)
      .sort((a, b) => b.created_at - a.created_at)
    return [...withOrder, ...withoutOrder]
  }, [sessions, orderOverride])

  const filtered = useMemo(() => {
    if (!query.trim()) return sorted
    const q = query.toLowerCase()
    return sorted.filter(s => (s.title || '').toLowerCase().includes(q))
  }, [sorted, query])

  const visible = filtered.slice(0, SESSIONS_LIMIT)

  const persistOrder = useCallback((next: Record<string, number>) => {
    setOrderOverride(next)
    try { window.localStorage.setItem(SESSIONS_ORDER_KEY, JSON.stringify(next)) } catch { /* noop */ }
  }, [])

  const handleDrop = useCallback((targetId: string) => {
    if (!draggedId || draggedId === targetId) return
    setDraggedId(null)
    const ids = sorted.map(s => s.id)
    const fromIdx = ids.indexOf(draggedId)
    const toIdx = ids.indexOf(targetId)
    if (fromIdx === -1 || toIdx === -1) return
    // Rebuild order map based on new sequence
    const reordered = [...ids]
    reordered.splice(fromIdx, 1)
    reordered.splice(toIdx, 0, draggedId)
    const next: Record<string, number> = {}
    reordered.forEach((id, idx) => { next[id] = idx })
    persistOrder(next)
  }, [draggedId, sorted, persistOrder])

  return (
    <div className="mb-lg">
      <div className="flex items-center justify-between px-2 mb-xs">
        <span className="font-label-sm text-label-sm text-on-surface-variant uppercase tracking-wider">
          {intl.formatMessage({ id: 'sidebar.sessions.title' })}
        </span>
        <span className="font-label-sm text-label-sm text-outline-variant">
          {filtered.length}{filtered.length !== sessions.length ? `/${sessions.length}` : ''}
        </span>
      </div>
      <input
        type="search"
        value={query}
        onChange={e => setQuery(e.target.value)}
        placeholder={intl.formatMessage({ id: 'sidebar.sessions.search.placeholder' })}
        aria-label={intl.formatMessage({ id: 'sidebar.sessions.search.aria' })}
        className="w-full mb-xs px-2 py-1 rounded-md bg-surface-container-lowest border border-outline-variant/30 font-label-md text-label-md text-on-surface placeholder:text-outline-variant focus-visible:border-primary focus-visible:ring-2 focus-visible:ring-primary/30"
      />
      {visible.length === 0 ? (
        <div className="px-2 py-3 text-center font-label-sm text-label-sm text-outline-variant">
          {intl.formatMessage({ id: 'sidebar.sessions.noResults' })}
        </div>
      ) : (
        <div className="space-y-0.5" role="list" aria-label={intl.formatMessage({ id: 'sidebar.sessions.list.aria' })}>
          {visible.map((session) => {
            const isActive = session.id === currentSessionId
            const isDragging = session.id === draggedId
            return (
              <div
                key={session.id}
                role="listitem"
                draggable
                onDragStart={() => setDraggedId(session.id)}
                onDragOver={e => e.preventDefault()}
                onDrop={() => handleDrop(session.id)}
                onClick={() => {
                  switchSession(session.id)
                  if (closeMobile) closeMobile()
                }}
                className={cn(
                  'w-full text-left px-3 py-2 rounded-lg font-label-md text-label-md transition-all duration-200 flex items-center gap-2 cursor-pointer select-none',
                  isActive
                    ? 'bg-primary/10 text-primary font-bold'
                    : 'text-on-surface-variant hover:bg-surface-container-low hover:text-primary',
                  isDragging && 'opacity-40'
                )}
                title={session.title}
              >
                <span
                  className="material-symbols-outlined text-[14px] text-outline-variant shrink-0"
                  aria-hidden="true"
                >drag_indicator</span>
                <span className="flex-1 truncate">{session.title || intl.formatMessage({ id: 'sidebar.sessions.untitled' })}</span>
              </div>
            )
          })}
        </div>
      )}
    </div>
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
  const { status, createSession, sessions, currentSessionId, switchSession, createSessionInWorktree } = useApp();
  const intl = useIntl();
  const { stats: triageStats, refresh: refreshTriageStats } = useTriageStats();

  // Poll triage stats every 30 seconds
  useEffect(() => {
    const interval = setInterval(() => {
      refreshTriageStats();
    }, 30000);
    return () => clearInterval(interval);
  }, [refreshTriageStats]);

  const handleMouseDown = useCallback((e: React.MouseEvent) => {
    e.preventDefault()
    dragging.current = true
    document.body.style.cursor = 'col-resize'
    document.body.style.userSelect = 'none'
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
      {/* Drag handle */}
      <div
        className="absolute right-0 top-0 bottom-0 w-1 cursor-col-resize hover:bg-primary/30 active:bg-primary/50 transition-colors z-10"
        aria-label={intl.formatMessage({ id: 'nav.resize.aria' })}
        title={intl.formatMessage({ id: 'nav.resize.title' })}
        onMouseDown={handleMouseDown}
      />
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

      {sessions.length > 0 && (
        <SessionsSection
          sessions={sessions}
          currentSessionId={currentSessionId}
          switchSession={switchSession}
          closeMobile={mobile ? closeMobile : undefined}
        />
      )}

      <nav aria-label={intl.formatMessage({ id: 'nav.mainNav.aria' })} className="flex-1 space-y-1">
        <ScrollArea className="h-full">
        <NavLink to="/chat" className={getNavClass} onClick={handleNavClick}>
           <span className="material-symbols-outlined">chat_bubble</span>
           <span className="flex-1">{intl.formatMessage({ id: 'nav.chat' })}</span>
           <kbd className="text-[10px] px-1.5 py-0.5 rounded bg-surface-container-high text-on-surface-variant font-mono opacity-60">{formatShortcut('1')}</kbd>
        </NavLink>
        <NavLink to="/tasks" className={getNavClass} onClick={handleNavClick}>
           <span className="material-symbols-outlined">task_alt</span>
           <span className="flex-1">{intl.formatMessage({ id: 'nav.scheduled' })}</span>
           <kbd className="text-[10px] px-1.5 py-0.5 rounded bg-surface-container-high text-on-surface-variant font-mono opacity-60">{formatShortcut('2')}</kbd>
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
        <button
          onClick={toggleMode}
          className="w-full flex items-center justify-between gap-3 px-4 py-2 rounded-lg font-label-md text-[12px] text-on-surface-variant hover:bg-surface-container-low hover:text-primary cursor-pointer transition-all"
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
          <span className="text-[10px] uppercase tracking-wider text-outline-variant">
            {intl.formatMessage({ id: mode === 'simple' ? 'nav.simpleMode.badge' : 'nav.devMode.badge' })}
          </span>
        </button>
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
             <SubNavLink to="/settings/billing" labelId="nav.usageBilling" />
             <SubNavLink to="/settings/advanced" labelId="nav.advanced" />
             <SubNavLink to="/settings/notifications" labelId="nav.notifications" />
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
