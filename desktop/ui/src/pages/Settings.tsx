import { NavLink, Outlet } from 'react-router-dom';
import { useIntl } from 'react-intl';
import { cn } from '@/lib/utils';
import { useSidebarMode } from '@/components/Sidebar';

// Review 2026-09-16 (UI-review §P1): the eight settings panes were reachable
// only through the Sidebar's 设置 disclosure — users bouncing between e.g.
// Models and Theme had to detour through the sidebar every time. This layout
// adds the in-page section nav competitors (Codex / Claude Desktop) have.
// Labels reuse the sidebar's `nav.*` keys so wording can't drift.
//
// 2026-09 dedup: the sidebar's disclosure was retired; this rail is now the
// only section switcher. 高级 stays dev-gated here — same contract as before
// (Sidebar.tsx:386-388 historical).

const SECTIONS: Array<{ to: string; labelId: string; icon: string }> = [
  { to: '/settings/general', labelId: 'nav.general', icon: 'tune' },
  { to: '/settings/theme', labelId: 'nav.theme', icon: 'palette' },
  { to: '/settings/models', labelId: 'nav.models', icon: 'smart_toy' },
  { to: '/settings/permissions', labelId: 'nav.permissions', icon: 'shield' },
  { to: '/settings/advanced', labelId: 'nav.advanced', icon: 'developer_mode' },
  { to: '/settings/notifications', labelId: 'nav.notifications', icon: 'notifications' },
  { to: '/settings/connections', labelId: 'nav.connections', icon: 'cloud' },
  { to: '/settings/remotes', labelId: 'nav.remotes', icon: 'settings_remote' },
];

export default function Settings() {
  const intl = useIntl();
  // The old sidebar disclosure dev-gated 高级 — keep that contract now that
  // this rail is the only section nav.
  const [mode] = useSidebarMode();
  const sections = mode === 'dev' ? SECTIONS : SECTIONS.filter(s => s.to !== '/settings/advanced');
  return (
    <div className="flex-1 h-full w-full bg-background flex flex-col md:flex-row min-h-0">
      {/* Section nav: horizontal scroll tabs on phones, left rail on desktop */}
      <nav
        aria-label={intl.formatMessage({ id: 'settings.section.aria' })}
        className="shrink-0 md:w-56 border-b md:border-b-0 md:border-r border-outline-variant/20 px-md md:px-md py-sm md:py-xl flex md:flex-col gap-xs overflow-x-auto md:overflow-y-auto"
      >
        {sections.map((s) => (
          <NavLink
            key={s.to}
            to={s.to}
            className={({ isActive }) =>
              cn(
                'flex items-center gap-sm whitespace-nowrap rounded-lg px-md py-sm font-label-md transition-colors cursor-pointer',
                isActive
                  ? 'bg-primary/10 text-primary font-medium'
                  : 'text-on-surface-variant hover:bg-surface-container-high hover:text-on-surface',
              )
            }
          >
            <span className="material-symbols-outlined icon-md" aria-hidden="true">{s.icon}</span>
            {intl.formatMessage({ id: s.labelId })}
          </NavLink>
        ))}
      </nav>
      <div className="flex-1 overflow-y-auto min-h-0 min-w-0">
        <div className="max-w-[1000px] mx-auto px-lg py-xl animate-in fade-in duration-700 pb-8">
          <Outlet />
        </div>
      </div>
    </div>
  );
}
