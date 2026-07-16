import { Outlet } from 'react-router-dom';
import { Sidebar } from './Sidebar';
import { Header } from './Header';

export function Layout() {
  return (
    <div className="bg-background text-on-surface font-body-md overflow-hidden min-h-screen">
      <Sidebar />
      <Header />
      <main className="ml-[280px] pt-16 pb-8 h-screen flex flex-col relative w-[calc(100%-280px)]">
        <Outlet />
      </main>
      <footer className="fixed bottom-0 right-0 left-[280px] h-8 bg-surface-container-low/90 backdrop-blur-sm border-t border-outline-variant/20 flex items-center justify-between px-lg z-40">
        <span className="font-label-sm text-label-sm text-on-surface-variant flex items-center gap-xs">
          Resource Usage: 1.2k tokens | Compute: 0.04s
        </span>
        <div className="flex items-center gap-md">
          <span className="font-label-sm text-label-sm text-primary flex items-center gap-xs">
            <span className="w-2 h-2 rounded-full bg-secondary animate-pulse"></span>
            Active Agents: ResearchBot, AutoCoder
          </span>
        </div>
      </footer>
    </div>
  );
}
