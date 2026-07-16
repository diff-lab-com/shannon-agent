import { useState } from 'react';
import { NavLink, useLocation } from 'react-router-dom';
import { Button } from '@/components/ui/button';
import { ScrollArea } from '@/components/ui/scroll-area';
import { cn } from '../lib/utils';

export function Sidebar() {
  const [opcOpen, setOpcOpen] = useState(true);
  const [extensionsOpen, setExtensionsOpen] = useState(true);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const location = useLocation();
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

  const getSubNavClass = ({ isActive }: { isActive: boolean }) =>
    cn(
      "flex items-center px-4 py-2 rounded-lg font-label-md text-[13px] transition-all duration-200",
      isActive
        ? "text-primary font-bold"
        : "text-on-surface-variant hover:text-primary"
    );

  const getSettingsClass = ({ isActive }: { isActive: boolean }) =>
    cn(
      "flex items-center gap-3 px-4 py-3 rounded-xl font-label-md text-label-md transition-all duration-300",
      isActive
        ? "bg-primary/10 text-primary font-bold shadow-sm"
        : "text-on-surface-variant hover:bg-surface-container-low hover:text-primary hover:-translate-y-0.5"
    );

  return (
    <aside className="fixed left-0 top-0 h-full w-[280px] bg-white/70 backdrop-blur-[20px] border-r border-outline-variant/30 flex flex-col py-lg px-md z-50 shadow-[4px_0_24px_-12px_rgba(0,0,0,0.1)]">
      <div className="flex items-center gap-3 mb-xl px-2">
        <div className="w-10 h-10 rounded-xl bg-primary flex items-center justify-center text-white shadow-lg shadow-primary/30">
          <span className="material-symbols-outlined" style={{fontVariationSettings: "'FILL' 1"}}>hub</span>
        </div>
        <div>
          <h1 className="font-headline-md text-[20px] font-bold text-primary leading-tight">Aether</h1>
          <p className="font-body-sm text-[12px] text-on-surface-variant leading-none">Effortless Intelligence</p>
        </div>
      </div>

      <Button className="mb-lg w-full py-3 px-4 bg-primary text-white rounded-xl font-bold flex items-center justify-center gap-2 hover:shadow-lg hover:shadow-primary/30 active:scale-95 transition-all">
        <span className="material-symbols-outlined text-[20px]">add</span>
        <span>New Request</span>
      </Button>

      <nav className="flex-1 space-y-1">
        <ScrollArea className="h-full">
        <NavLink to="/chat" className={getNavClass}>
           <span className="material-symbols-outlined">chat_bubble</span>
           <span>Chat</span>
        </NavLink>
        <NavLink to="/goals" className={getNavClass}>
           <span className="material-symbols-outlined">ads_click</span>
           <span>Goals</span>
        </NavLink>
        <NavLink to="/tasks" className={getNavClass}>
           <span className="material-symbols-outlined">task_alt</span>
           <span>Tasks</span>
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
            <span className="material-symbols-outlined text-[20px] transition-transform duration-200" style={{ transform: extensionsOpen ? 'rotate(180deg)' : 'rotate(0deg)' }}>expand_more</span>
          </Button>
          
          {extensionsOpen && (
            <div className="pl-4 pr-2 space-y-1 mt-1 transition-all">
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
            onClick={() => setOpcOpen(!opcOpen)}
            className={cn("w-full flex items-center justify-between gap-3 px-4 py-3 rounded-lg font-label-md text-label-md transition-all duration-200", isOpcActive ? "bg-primary/10 text-primary font-bold" : "text-on-surface-variant hover:bg-surface-container-high/50 hover:text-primary")}
          >
            <div className="flex items-center gap-3">
              <span>OPC</span>
              <span className="text-[9px] bg-primary text-white px-1.5 py-0.5 rounded uppercase font-bold tracking-wider">Experiment</span>
            </div>
            <span className="material-symbols-outlined text-[20px] transition-transform duration-200" style={{ transform: opcOpen ? 'rotate(180deg)' : 'rotate(0deg)' }}>expand_more</span>
          </Button>
          
          {opcOpen && (
            <div className="pl-4 pr-2 space-y-1 mt-1 transition-all">
               <NavLink to="/opc" className={getSubNavClass}>
                  {({ isActive }) => (
                    <>
                      <span className={cn("w-1.5 h-1.5 rounded-full mr-3 shrink-0", isActive ? "bg-primary" : "bg-outline-variant")}></span>
                      Aether Intelligence
                    </>
                  )}
               </NavLink>
               <Button variant="ghost" className="w-full flex items-center px-4 py-2 rounded-lg font-label-md text-[13px] text-on-surface-variant hover:text-primary transition-all duration-200">
                  <span className="w-1.5 h-1.5 rounded-full bg-outline-variant mr-3 shrink-0"></span>
                  Project Hermes
               </Button>
               <Button variant="ghost" className="w-full flex items-center px-4 py-2 rounded-lg font-label-md text-[13px] text-on-surface-variant hover:text-primary transition-all duration-200">
                  <span className="w-1.5 h-1.5 rounded-full bg-outline-variant mr-3 shrink-0"></span>
                  Digital Nomad Studio
               </Button>
               <Button variant="ghost" className="w-full flex items-center px-2 py-2 rounded-lg font-label-md text-[13px] italic text-primary hover:bg-primary/5 transition-all duration-200">
                  <span className="material-symbols-outlined text-[16px] mr-2">add</span>
                  New OPC
               </Button>
            </div>
          )}
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
          <span className="material-symbols-outlined text-[20px] transition-transform duration-200" style={{ transform: settingsOpen ? 'rotate(180deg)' : 'rotate(0deg)' }}>expand_more</span>
        </Button>

        {settingsOpen && (
          <div className="pl-4 pr-2 space-y-1 mt-1 transition-all">
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
      </div>
    </aside>
  );
}
