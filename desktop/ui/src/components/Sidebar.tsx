import { useState, useCallback, useEffect, useRef, memo } from 'react';
import { NavLink, useNavigate } from 'react-router-dom';
import { useIntl } from 'react-intl';
import { Button } from '@/components/ui/button';
import { ScrollArea } from '@/components/ui/scroll-area';
import { DropdownMenu, type DropdownMenuItem } from '@/components/ui/dropdown-menu';
import EmptyState from './ui/empty-state';
import { WELCOME_EXAMPLES } from './welcomeExamples';
import { cn } from '../lib/utils';
import { useSessions } from '@/context/SessionContext';
import { useCatalog } from '@/context/CatalogContext';
import { SessionsSection } from './SidebarSessions';
import { useSidebar } from './Layout';
import { useInboxStats } from '@/hooks/inbox';

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

/* 2026-09 review — the nav is rebuilt on ZCode's minimal pattern:
 * four flat entries (对话 / 自动化 / 收件箱 / 扩展市场) plus 记忆, with the
 * dev-only extras (用量 / OPC) folded behind the dev toggle. The old
 * Work/Resources/Experiments disclosure groups tripled the chrome per
 * destination and the nested Extensions disclosure hid Skills/Agents behind
 * two folds. Every row is zoom-safe by construction: icon shrink-0, label
 * flex-1 min-w-0 truncate, whitespace-nowrap — page zoom (Ctrl+=) can never
 * wrap or collide the row (the old fixed-px rows broke into two lines and
 * shoved the kbd chips out of the rail). Shortcuts moved into tooltips. */

const getNavClass = ({ isActive }: { isActive: boolean }) =>
  cn(
    "flex items-center gap-2.5 px-3 py-2 rounded-xl font-label-md text-[13px] transition-all duration-200 whitespace-nowrap min-w-0",
    isActive
      ? "text-on-surface bg-primary/10 font-bold"
      : "text-on-surface-variant hover:bg-surface-container-low hover:text-primary"
  );

/** One flat nav row: fixed icon + truncating label + optional trailing slot. */
function NavRow({ to, icon, labelId, titleId, trail, onNavigate }: {
  to: string
  icon: string
  labelId: string
  titleId?: string
  trail?: React.ReactNode
  onNavigate?: () => void
}) {
  const intl = useIntl()
  const label = intl.formatMessage({ id: labelId })
  return (
    <NavLink to={to} className={getNavClass} title={intl.formatMessage({ id: titleId ?? labelId })} aria-label={intl.formatMessage({ id: titleId ?? labelId })} onClick={onNavigate}>
      {({ isActive }) => (
        <>
          <span className="material-symbols-outlined text-[20px] shrink-0" style={{ fontVariationSettings: isActive ? "'FILL' 1" : undefined }} aria-hidden="true">{icon}</span>
          <span className="flex-1 min-w-0 truncate">{label}</span>
          {trail}
        </>
      )}
    </NavLink>
  )
}

export const Sidebar = memo(function Sidebar({ mobile, open = true }: { mobile?: boolean; open?: boolean }) {
  const { close: closeMobile } = useSidebar();
  const [mode, toggleMode] = useSidebarMode();
  const [width, setWidth] = useState(() => {
    const stored = localStorage.getItem(STORAGE_KEY)
    return stored ? Math.min(MAX_W, Math.max(MIN_W, parseInt(stored, 10) || DEFAULT_W)) : DEFAULT_W
  });
  const dragging = useRef(false);
  const navigate = useNavigate();
  // P2-⑩: split-"New" dropdown (goal / routine entry points).
  const [newMenuOpen, setNewMenuOpen] = useState(false);
  const { createSession, sessions, sessionActivity, goalRunsBySession, currentSessionId, switchSession, renameSession, deleteSession, createSessionInWorktree } = useSessions();
  const { status } = useCatalog();
  const intl = useIntl();
  const newMenuItems: DropdownMenuItem[] = [
    { id: 'goal', label: intl.formatMessage({ id: 'nav.new.goal' }), icon: 'flag', onSelect: () => { setNewMenuOpen(false); navigate('/tasks') } },
    { id: 'routine', label: intl.formatMessage({ id: 'nav.new.routine' }), icon: 'event_repeat', onSelect: () => { setNewMenuOpen(false); navigate('/tasks') } },
  ];
  const { stats: inboxStats, refresh: refreshInboxStats } = useInboxStats();

  useEffect(() => {
    if (typeof document === 'undefined') return;
    const onVisibility = () => { if (!document.hidden) refreshInboxStats(); };
    refreshInboxStats();
    document.addEventListener('visibilitychange', onVisibility);
    return () => { document.removeEventListener('visibilitychange', onVisibility); };
  }, [refreshInboxStats]);

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
      "fixed left-0 top-0 h-full bg-surface-container-lowest/85 border-r border-outline-variant/30 flex flex-col py-lg px-md shadow-[4px_0_24px_-12px_color-mix(in_srgb,var(--color-inverse-surface)_15%,transparent)] transition-transform duration-300",
      mobile
        ? cn("z-drawer w-[280px]", open ? "translate-x-0" : "-translate-x-full")
        : "z-20",
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
        className="group absolute right-0 top-0 bottom-0 w-2 cursor-col-resize z-raised"
        aria-label={intl.formatMessage({ id: 'nav.resize.aria' })}
        title={intl.formatMessage({ id: 'nav.resize.title' })}
        onMouseDown={handleMouseDown}
        onDoubleClick={resetWidth}
        onKeyDown={handleResizeKey}
      >
        <div className="absolute right-0 top-0 bottom-0 w-1 transition-colors group-hover:bg-primary/30 group-focus-visible:bg-primary/30 group-active:bg-primary/50" />
      </div>
      <div className="flex items-center gap-3 mb-xl px-2 min-w-0">
        {/* U8: brand mark `cognitive` (filled) — a knowledge-graph knot reads as
            "connected intelligence" and nods to Shannon's information theory. */}
        <div className="w-9 h-9 rounded-xl bg-primary flex items-center justify-center text-on-primary shadow-lg shadow-primary/30 shrink-0">
          <span className="material-symbols-outlined" style={{fontVariationSettings: "'FILL' 1"}}>cognitive</span>
        </div>
        <div className="min-w-0">
          <h1 className="font-headline-md text-[18px] font-bold text-on-surface leading-tight truncate">Shannon</h1>
          <p className="font-body-sm text-[11px] text-on-surface-variant leading-none truncate">
            {intl.formatMessage({ id: 'nav.tagline' })}
          </p>
        </div>
      </div>

      {/* P2-⑩ (ZCode delta): "New" is a split button — chat stays the
          primary action, goal/routine creation is one click away instead of
          a detour through the Tasks page. */}
      <div className="mb-xs w-full flex gap-1">
        <Button
          aria-label={intl.formatMessage({ id: 'nav.newChat.aria' })}
          className="flex-1 min-w-0 py-2.5 px-3 bg-primary text-on-primary rounded-xl font-bold flex items-center justify-center gap-2 hover:shadow-lg hover:shadow-primary/30 active:scale-95 transition-all"
          onClick={createSession}
        >
          <span className="material-symbols-outlined icon-md shrink-0">add</span>
          <span className="truncate">{intl.formatMessage({ id: 'nav.newChat' })}</span>
        </Button>
        <div className="relative shrink-0">
          <Button
            variant="outline"
            aria-label={intl.formatMessage({ id: 'nav.new.more.aria' })}
            title={intl.formatMessage({ id: 'nav.new.more.aria' })}
            aria-haspopup="menu"
            className="h-full px-2 rounded-xl border-outline-variant/30 bg-surface-container-lowest/60 text-on-surface-variant hover:bg-surface-container-low hover:text-primary transition-all"
            onClick={() => setNewMenuOpen(v => !v)}
          >
            <span className="material-symbols-outlined icon-md" aria-hidden="true">unfold_more</span>
          </Button>
          {newMenuOpen && (
            <DropdownMenu
              open
              onClose={() => setNewMenuOpen(false)}
              items={newMenuItems}
              align="end"
              className="w-44 min-w-0"
              ariaLabel={intl.formatMessage({ id: 'nav.new.more.aria' })}
            />
          )}
        </div>
      </div>
      {mode === 'dev' && (
      <Button
        variant="ghost"
        aria-label={intl.formatMessage({ id: 'sidebar.worktree.new.aria' })}
        title={intl.formatMessage({ id: 'sidebar.worktree.new.title' })}
        className="mb-xs w-full py-2 px-3 text-on-surface-variant hover:text-primary rounded-lg font-label-md text-[13px] flex items-center justify-center gap-1.5 hover:bg-surface-container-low transition-all min-w-0"
        onClick={createSessionInWorktree}
      >
        <span className="material-symbols-outlined icon-sm shrink-0">account_tree</span>
        <span className="truncate">{intl.formatMessage({ id: 'sidebar.worktree.new' })}</span>
      </Button>
      )}

      {/* U1: the session rail is the app's only session list — organized by
          project folder or by time (the toggle lives inside the rail, ZCode
          分组/项目 style). Takes the remaining vertical space. */}
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
            sessionActivity={sessionActivity}
            goalRunsBySession={goalRunsBySession}
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
          <div className="space-y-0.5">
            <NavRow to="/chat" icon="chat_bubble" labelId="nav.chat" titleId="nav.chat" onNavigate={handleNavClick} />
            <NavRow to="/tasks" icon="task_alt" labelId="nav.scheduled" titleId="nav.scheduled" onNavigate={handleNavClick} />
            <NavRow
              to="/triage"
              icon="inbox"
              labelId="nav.triage"
              titleId="nav.triage.aria"
              onNavigate={handleNavClick}
              trail={inboxStats.pending > 0 ? (
                <span className="bg-error text-on-error text-[11px] font-bold px-1.5 py-0.5 rounded-full shrink-0">
                  {inboxStats.pending}
                </span>
              ) : undefined}
            />
            <NavRow to="/extensions/featured" icon="extension" labelId="nav.extensions" titleId="nav.extensions" onNavigate={handleNavClick} />
            <NavRow to="/memory" icon="psychology" labelId="nav.memory" titleId="nav.memory" onNavigate={handleNavClick} />
            {mode === 'dev' && (
              <>
                <NavRow to="/usage" icon="monitoring" labelId="nav.usage" titleId="nav.usage" onNavigate={handleNavClick} />
                <NavRow to="/opc" icon="auto_awesome" labelId="nav.opc" titleId="nav.opc" onNavigate={handleNavClick} />
              </>
            )}
          </div>
        </ScrollArea>
      </nav>

      <div className="mt-auto pt-lg border-t border-outline-variant/20 space-y-0.5">
        <Button
          variant="ghost"
          onClick={toggleMode}
          className="w-full justify-between gap-3 px-3 py-2 rounded-lg font-label-md text-[12px] text-on-surface-variant hover:bg-surface-container-low hover:text-primary cursor-pointer transition-all h-auto min-w-0 whitespace-nowrap"
          aria-label={intl.formatMessage({ id: mode === 'simple' ? 'nav.simpleMode.aria' : 'nav.devMode.aria' })}
          aria-pressed={mode === 'dev'}
          title={intl.formatMessage({ id: mode === 'simple' ? 'nav.simpleMode.title' : 'nav.devMode.title' })}
        >
          <div className="flex items-center gap-2 min-w-0">
            <span className="material-symbols-outlined text-[18px] shrink-0">{mode === 'simple' ? 'tune' : 'dashboard_customize'}</span>
            <span className="truncate">
              {intl.formatMessage({ id: mode === 'simple' ? 'nav.modeLabel.simple' : 'nav.modeLabel.dev' })}
            </span>
          </div>
          <span className="text-[10px] uppercase tracking-wider text-on-surface-variant shrink-0">
            {intl.formatMessage({ id: mode === 'simple' ? 'nav.simpleMode.badge' : 'nav.devMode.badge' })}
          </span>
        </Button>
        {/* 2026-09 dedup: one flat Settings entry — the section switcher
    lives on the Settings page rail (pages/Settings.tsx). The old
    disclosure duplicated it and drifted (dev-gated 高级 here only). */}
        <NavRow to="/settings" icon="settings" labelId="nav.settings" onNavigate={handleNavClick} />

        {/* Status bar */}
        {status && (
          <div className="mt-sm px-2 py-sm flex items-center gap-sm text-label-sm text-on-surface-variant min-w-0">
            <span className="w-2 h-2 rounded-full bg-tertiary shrink-0"></span>
            <span className="truncate">{status.model}</span>
          </div>
        )}
      </div>
    </aside>
  );
});
