import React from 'react';
import { useLocation } from 'react-router-dom';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';

export function Header() {
  const location = useLocation();
  
  let title = "Chat";
  if (location.pathname.includes('/settings')) title = "Settings";
  if (location.pathname.includes('/tasks')) title = "Tasks";
  if (location.pathname.includes('/goals')) title = "Goals";
  if (location.pathname.includes('/extensions')) title = "Extensions";
  if (location.pathname.includes('/opc/task')) title = "Revamp Landing Page Hero";
  else if (location.pathname.includes('/opc')) title = "One Person Company";

  const isOpc = location.pathname.includes('/opc') && !location.pathname.includes('/opc/task');
  const isOpcTask = location.pathname.includes('/opc/task');

  return (
    <header className="fixed top-0 right-0 left-[280px] z-40 flex justify-between items-center h-16 px-lg bg-surface/80 backdrop-blur-md shadow-sm border-b border-outline-variant/10">
      <div className="flex items-center gap-md relative w-full overflow-hidden">
        {isOpcTask ? (
          <div className="flex items-center gap-2">
            <span className="material-symbols-outlined text-primary text-[28px]">auto_awesome</span>
            <h2 className="font-headline-md text-[24px] font-extrabold text-primary whitespace-nowrap">{title}</h2>
            <span className="material-symbols-outlined text-on-surface-variant text-[18px] cursor-pointer hover:text-primary transition-colors ml-1">edit</span>
          </div>
        ) : (
          <h2 className="font-headline-md text-[24px] font-extrabold text-on-surface whitespace-nowrap">{title}</h2>
        )}
        
        {isOpc && (
          <div className="flex-1 max-w-[400px] ml-auto mr-lg relative hidden md:block">
            <Input
              type="text"
              placeholder="Search proposals..."
              className="w-full bg-surface-container-low border-none rounded-full py-2 pl-4 pr-10 text-sm font-body-md focus:ring-2 focus:ring-primary/20 transition-all outline-none"
            />
            <span className="material-symbols-outlined absolute right-3 top-1/2 -translate-y-1/2 text-on-surface-variant text-[20px]">search</span>
          </div>
        )}

        {isOpcTask && (
          <div className="ml-auto mr-lg flex items-center gap-2 bg-surface-container-low px-3 py-1.5 rounded-full border border-outline-variant/20 shrink-0">
             <span className="w-2 h-2 rounded-full bg-green-500"></span>
             <span className="font-label-sm text-[12px] text-on-surface-variant whitespace-nowrap">Sync Status: Realtime</span>
          </div>
        )}
      </div>
      <div className="flex items-center gap-lg shrink-0 pl-4 border-l border-outline-variant/20 md:border-none md:pl-0">
        <div className="flex gap-md mr-md">
          <Button variant="ghost" className="text-on-surface-variant hover:text-primary transition-all active:scale-95">
            <span className="material-symbols-outlined text-[20px]" data-icon="notifications">notifications</span>
          </Button>
          {!isOpc && (
            <Button variant="ghost" className="text-on-surface-variant hover:text-primary transition-all active:scale-95">
              <span className="material-symbols-outlined text-[20px]" data-icon="help_outline">help_outline</span>
            </Button>
          )}
        </div>
        <div className="h-8 w-8 rounded-full overflow-hidden bg-surface-container flex items-center justify-center ring-2 ring-primary/10">
          <img 
            alt="User profile" 
            className="w-full h-full object-cover" 
            src="https://lh3.googleusercontent.com/aida-public/AB6AXuAYwvfZK43KX11aBZynal1grIs-ypQi8kHp6dfajmo8KpB_TvKfY3yeXX--euv0WroSfo82IYDaV5i3I62l1VmJzazQN5oQiUq55ZB1SyfSR8DbOCavVPwFWB8e8J1KfmJF9mJmq8QOo85By882nrNejdX7RrdnrEIk-YCWq-b49bV28gQ4rUHEHzvodgzbYD5DVTaiJuNUwAOfaQ5C0j72JDQUxQYDYeBjRYhK9ZHQ_PVpyzGzVd6vAmZl-KQcfXBCHs8sy66msD8" 
          />
        </div>
      </div>
    </header>
  );
}
