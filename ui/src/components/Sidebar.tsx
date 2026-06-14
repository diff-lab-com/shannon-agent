import { useState, useCallback, useEffect, useRef, memo } from 'react';
import { NavLink, useLocation } from 'react-router-dom';
import { Button } from '@/components/ui/button';
import { ScrollArea } from '@/components/ui/scroll-area';
import { cn } from '../lib/utils';
import { useApp } from '@/context/AppContext';
import { useSidebar } from './Layout';

const MIN_W = 200
const MAX_W = 400
const DEFAULT_W = 280
const STORAGE_KEY = 'shannon-sidebar-width'

export const Sidebar = memo(function Sidebar({ mobile }: { mobile?: boolean }) {
  const { close: closeMobile } = useSidebar();
  const [opcOpen, setOpcOpen] = useState(true);
  const [extensionsOpen, setExtensionsOpen] = useState(true);
  const [automationOpen, setAutomationOpen] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [width, setWidth] = useState(() => {
    const stored = localStorage.getItem(STORAGE_KEY)
    return stored ? Math.min(MAX_W, Math.max(MIN_W, parseInt(stored, 10) || DEFAULT_W)) : DEFAULT_W
  });
  const dragging = useRef(false);
  const location = useLocation();
  const { status, createSession } = useApp();

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
  const isAutomationActive = location.pathname.includes('/routines') || location.pathname.includes('/hooks') || location.pathname.includes('/profiles');
  const isSettingsActive = location.pathname.includes('/settings');

  const getNavClass = ({ isActive }: { isActive: boolean }) =>
    cn(
      "flex items-center gap-3 px-4 py-3 rounded-xl font-label-md text-label-md transition-all duration-300",
      isActive
        ? "text-primary bg-primary/10 font-bold shadow-sm"
        : "text-on-surface-variant hover:bg-surface-container-low hover:text-primary hover:-translate-y-0.5"
    );

  const getSubNavClass = ({ isActive }: { isActive: boolean }) =>
    cn(
      "flex items-center px-4 py-2 rounded-lg font-label-md text-[13px] transition-all duration-200",
      isActive
        ? "text-primary font-bold"
        : "text-on-surface-variant hover:text-primary"
    );

  const handleNavClick = () => { if (mobile) closeMobile() }

  return (
    <aside data-sidebar className={cn(
      "fixed left-0 top-0 h-full bg-surface-container-lowest/70 backdrop-blur-[20px] border-r border-outline-variant/30 flex flex-col py-lg px-md shadow-[4px_0_24px_-12px_rgba(0,0,0,0.1)] transition-transform duration-300",
      mobile ? "z-[70] w-[280px]" : "z-50",
    )} style={mobile ? undefined : { width }}>
      {/* Drag handle */}
      <div
        className="absolute right-0 top-0 bottom-0 w-1 cursor-col-resize hover:bg-primary/30 active:bg-primary/50 transition-colors z-10"
        aria-label="Resize sidebar" title="Drag to resize sidebar"
        onMouseDown={handleMouseDown}
      />
      <div className="flex items-center gap-3 mb-xl px-2">
        <div className="w-10 h-10 rounded-xl bg-primary flex items-center justify-center text-on-primary shadow-lg shadow-primary/30">
          <span className="material-symbols-outlined" style={{fontVariationSettings: "'FILL' 1"}}>hub</span>
        </div>
        <div>
          <h1 className="font-headline-md text-[20px] font-bold text-primary leading-tight">Shannon</h1>
          <p className="font-body-sm text-[12px] text-on-surface-variant leading-none">AI Code Assistant</p>
        </div>
      </div>

      <Button
        aria-label="New chat"
        className="mb-lg w-full py-3 px-4 bg-primary text-on-primary rounded-xl font-bold flex items-center justify-center gap-2 hover:shadow-lg hover:shadow-primary/30 active:scale-95 transition-all"
        onClick={createSession}
      >
        <span className="material-symbols-outlined text-[20px]">add</span>
        <span>New Chat</span>
      </Button>

      <nav aria-label="Main navigation" className="flex-1 space-y-1">
        <ScrollArea className="h-full">
        <NavLink to="/chat" className={getNavClass} onClick={handleNavClick}>
           <span className="material-symbols-outlined">chat_bubble</span>
           <span className="flex-1">Chat</span>
           <kbd className="text-[10px] px-1.5 py-0.5 rounded bg-surface-container-high text-on-surface-variant font-mono opacity-60">⌘1</kbd>
        </NavLink>
        <NavLink to="/goals" className={getNavClass} onClick={handleNavClick}>
           <span className="material-symbols-outlined">ads_click</span>
           <span className="flex-1">Goals</span>
           <kbd className="text-[10px] px-1.5 py-0.5 rounded bg-surface-container-high text-on-surface-variant font-mono opacity-60">⌘2</kbd>
        </NavLink>
        <NavLink to="/tasks" className={getNavClass} onClick={handleNavClick}>
           <span className="material-symbols-outlined">task_alt</span>
           <span className="flex-1">Scheduled</span>
           <kbd className="text-[10px] px-1.5 py-0.5 rounded bg-surface-container-high text-on-surface-variant font-mono opacity-60">⌘3</kbd>
        </NavLink>
        <NavLink to="/mission-control" className={getNavClass} onClick={handleNavClick}>
           <span className="material-symbols-outlined">dashboard</span>
           <span className="flex-1">Mission Control</span>
           <kbd className="text-[10px] px-1.5 py-0.5 rounded bg-surface-container-high text-on-surface-variant font-mono opacity-60">⌘4</kbd>
        </NavLink>
        <NavLink to="/triage" className={getNavClass} onClick={handleNavClick}>
           <span className="material-symbols-outlined">flag</span>
           <span className="flex-1">Triage</span>
        </NavLink>
        <div className="space-y-1">
          <Button
            variant="ghost"
            onClick={() => setExtensionsOpen(!extensionsOpen)}
            className={cn("w-full flex items-center justify-between gap-3 px-4 py-3 rounded-xl font-label-md text-label-md transition-all duration-300", isExtensionsActive ? "bg-primary/10 text-primary font-bold shadow-sm" : "text-on-surface-variant hover:bg-surface-container-low hover:text-primary hover:-translate-y-0.5")}
          >
            <div className="flex items-center gap-3">
              <span className="material-symbols-outlined">grid_view</span>
              <span>Extensions</span>
            </div>
            <span className="material-symbols-outlined text-[20px] transition-transform duration-200" style={{ transform: extensionsOpen ? 'rotate(180deg)' : 'rotate(0deg)' }} aria-hidden="true">expand_more</span>
          </Button>

          {extensionsOpen && (
            <div className="pl-4 pr-2 space-y-1 mt-1 transition-all" aria-label="Extensions">
               <NavLink to="/extensions/skills" className={getSubNavClass}>
                  {({ isActive }) => (
                    <>
                      <span className={cn("w-1.5 h-1.5 rounded-full mr-3 shrink-0", isActive ? "bg-primary" : "bg-outline-variant")}></span>
                      Skills
                    </>
                  )}
               </NavLink>
               <NavLink to="/extensions/agents" className={getSubNavClass}>
                  {({ isActive }) => (
                    <>
                      <span className={cn("w-1.5 h-1.5 rounded-full mr-3 shrink-0", isActive ? "bg-primary" : "bg-outline-variant")}></span>
                      My Agents
                    </>
                  )}
               </NavLink>
               <NavLink to="/extensions/datasources" className={getSubNavClass}>
                  {({ isActive }) => (
                    <>
                      <span className={cn("w-1.5 h-1.5 rounded-full mr-3 shrink-0", isActive ? "bg-primary" : "bg-outline-variant")}></span>
                      Data Sources
                    </>
                  )}
               </NavLink>
            </div>
          )}
        </div>

        <div className="space-y-1">
          <Button
            variant="ghost"
            onClick={() => setAutomationOpen(!automationOpen)}
            className={cn("w-full flex items-center justify-between gap-3 px-4 py-3 rounded-xl font-label-md text-label-md transition-all duration-300", isAutomationActive ? "bg-primary/10 text-primary font-bold shadow-sm" : "text-on-surface-variant hover:bg-surface-container-low hover:text-primary hover:-translate-y-0.5")}
          >
            <div className="flex items-center gap-3">
              <span className="material-symbols-outlined">bolt</span>
              <span>Automation</span>
            </div>
            <span className="material-symbols-outlined text-[20px] transition-transform duration-200" style={{ transform: automationOpen ? 'rotate(180deg)' : 'rotate(0deg)' }} aria-hidden="true">expand_more</span>
          </Button>

          {automationOpen && (
            <div className="pl-4 pr-2 space-y-1 mt-1 transition-all" aria-label="Automation">
               <NavLink to="/routines" className={getSubNavClass}>
                  {({ isActive }) => (
                    <>
                      <span className={cn("w-1.5 h-1.5 rounded-full mr-3 shrink-0", isActive ? "bg-primary" : "bg-outline-variant")}></span>
                      Routines
                    </>
                  )}
               </NavLink>
               <NavLink to="/hooks" className={getSubNavClass}>
                  {({ isActive }) => (
                    <>
                      <span className={cn("w-1.5 h-1.5 rounded-full mr-3 shrink-0", isActive ? "bg-primary" : "bg-outline-variant")}></span>
                      Hook Events
                    </>
                  )}
               </NavLink>
               <NavLink to="/profiles" className={getSubNavClass}>
                  {({ isActive }) => (
                    <>
                      <span className={cn("w-1.5 h-1.5 rounded-full mr-3 shrink-0", isActive ? "bg-primary" : "bg-outline-variant")}></span>
                      Profiles
                    </>
                  )}
               </NavLink>
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
              <span>OPC</span>
              <span className="text-[9px] bg-primary text-on-primary px-1.5 py-0.5 rounded uppercase font-bold tracking-wider">Experiment</span>
            </div>
            <span className="material-symbols-outlined text-[20px] transition-transform duration-200" style={{ transform: opcOpen ? 'rotate(180deg)' : 'rotate(0deg)' }} aria-hidden="true">expand_more</span>
          </Button>

          {opcOpen && (
            <div className="pl-4 pr-2 space-y-1 mt-1 transition-all">
               <NavLink to="/opc" className={getSubNavClass}>
                  {({ isActive }) => (
                    <>
                      <span className={cn("w-1.5 h-1.5 rounded-full mr-3 shrink-0", isActive ? "bg-primary" : "bg-outline-variant")}></span>
                      One Person Company
                    </>
                  )}
               </NavLink>
            </div>
          )}
        </div>

        <div className="space-y-1">
          <NavLink
            to="/quickfix"
            className={cn(
              "w-full flex items-center gap-3 px-4 py-3 rounded-lg font-label-md text-label-md transition-all duration-200",
              location.pathname === '/quickfix'
                ? "bg-primary/10 text-primary font-bold"
                : "text-on-surface-variant hover:bg-surface-container-high/50 hover:text-primary",
            )}
          >
            <span className="material-symbols-outlined text-[20px]">build</span>
            <span>Quick Fix</span>
          </NavLink>
          <NavLink
            to="/editor"
            className={cn(
              "w-full flex items-center gap-3 px-4 py-3 rounded-lg font-label-md text-label-md transition-all duration-200",
              location.pathname === '/editor'
                ? "bg-primary/10 text-primary font-bold"
                : "text-on-surface-variant hover:bg-surface-container-high/50 hover:text-primary",
            )}
          >
            <span className="material-symbols-outlined text-[20px]">code</span>
            <span>Editor</span>
          </NavLink>
        </div>
        </ScrollArea>
      </nav>

      <div className="mt-auto pt-lg border-t border-outline-variant/20 space-y-1">
        <Button
          variant="ghost"
          onClick={() => setSettingsOpen(!settingsOpen)}
          className={cn("w-full flex items-center justify-between gap-3 px-4 py-3 rounded-xl font-label-md text-label-md transition-all duration-300", isSettingsActive ? "bg-primary/10 text-primary font-bold shadow-sm" : "text-on-surface-variant hover:bg-surface-container-low hover:text-primary hover:-translate-y-0.5")}
        >
          <div className="flex items-center gap-3">
            <span className="material-symbols-outlined" style={{fontVariationSettings: "'FILL' 1"}}>settings</span>
            <span>Settings</span>
          </div>
          <span className="material-symbols-outlined text-[20px] transition-transform duration-200" style={{ transform: settingsOpen ? 'rotate(180deg)' : 'rotate(0deg)' }} aria-hidden="true">expand_more</span>
        </Button>

        {settingsOpen && (
          <div className="pl-4 pr-2 space-y-1 mt-1 transition-all" aria-label="Settings">
             <NavLink to="/settings/general" className={getSubNavClass}>
                {({ isActive }) => (
                  <>
                    <span className={cn("w-1.5 h-1.5 rounded-full mr-3 shrink-0", isActive ? "bg-primary" : "bg-outline-variant")}></span>
                    General
                  </>
                )}
             </NavLink>
             <NavLink to="/settings/theme" className={getSubNavClass}>
                {({ isActive }) => (
                  <>
                    <span className={cn("w-1.5 h-1.5 rounded-full mr-3 shrink-0", isActive ? "bg-primary" : "bg-outline-variant")}></span>
                    Theme
                  </>
                )}
             </NavLink>
             <NavLink to="/settings/models" className={getSubNavClass}>
                {({ isActive }) => (
                  <>
                    <span className={cn("w-1.5 h-1.5 rounded-full mr-3 shrink-0", isActive ? "bg-primary" : "bg-outline-variant")}></span>
                    Models
                  </>
                )}
             </NavLink>
             <NavLink to="/settings/billing" className={getSubNavClass}>
                {({ isActive }) => (
                  <>
                    <span className={cn("w-1.5 h-1.5 rounded-full mr-3 shrink-0", isActive ? "bg-primary" : "bg-outline-variant")}></span>
                    Usage & Billing
                  </>
                )}
             </NavLink>
             <NavLink to="/settings/advanced" className={getSubNavClass}>
                {({ isActive }) => (
                  <>
                    <span className={cn("w-1.5 h-1.5 rounded-full mr-3 shrink-0", isActive ? "bg-primary" : "bg-outline-variant")}></span>
                    Advanced
                  </>
                )}
             </NavLink>
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
