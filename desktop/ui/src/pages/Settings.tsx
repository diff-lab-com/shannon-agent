import { NavLink, Outlet } from 'react-router-dom';
import { useIntl } from 'react-intl';
import { cn } from '@/lib/utils';
import { useSidebarMode } from '@/components/Sidebar';

// Review 2026-09-16 (UI-review §P1): the settings panes were reachable only
// through the Sidebar's 设置 disclosure — users bouncing between e.g. Models
// and Theme had to detour through the sidebar every time. This layout adds
// the in-page section nav competitors (Codex / Claude Desktop) have. Labels
// reuse the sidebar's `nav.*` keys so wording can't drift.
//
// 2026-09 dedup: the sidebar's disclosure was retired; this rail is now the
// only section switcher. 高级 stays dev-gated here — same contract as before
// (Sidebar.tsx:386-388 historical).
//
// IA redesign 2026-10 (ADVERSARIAL-REVIEW §2): 11 sections → 8. 网络 merges
// into 连接 (bottom of ConnectionsPane), 会话 merges into 通用 (bottom of
// GeneralPane), 远程目标 merges into 连接 (middle of ConnectionsPane). The
// old deep links /settings/network|session|remotes redirect in App.tsx —
// no bookmark breaks. Labels still reuse existing `nav.*` keys: nav.theme
// is now displayed as 外观/Appearance and nav.connections as 连接/Connections
// (its only consumers are this rail + the command palette, both updated).
//
// Design-parity R1 (2026-10-08 §1): the rail gains the design's「设置」
// sec-title (12-settings.html:149; reuses nav.settings so the wording can't
// drift), and the content column narrows max-w-medium → max-w-narrow — the
// design pane is ~720px, and every pane component (General/Theme/Models)
// already assumed the 48rem reading width internally.

const SECTIONS: Array<{ to: string; labelId: string; icon: string }> = [
  { to: '/settings/general', labelId: 'nav.general', icon: 'tune' },
  { to: '/settings/theme', labelId: 'nav.theme', icon: 'palette' },
  { to: '/settings/models', labelId: 'nav.models', icon: 'smart_toy' },
  { to: '/settings/permissions', labelId: 'nav.permissions', icon: 'shield' },
  { to: '/settings/notifications', labelId: 'nav.notifications', icon: 'notifications' },
  { to: '/settings/connections', labelId: 'nav.connections', icon: 'cloud' },
  { to: '/settings/about', labelId: 'nav.about', icon: 'info' },
  // Dev-gated — filtered out below in simple mode; always rendered last.
  { to: '/settings/advanced', labelId: 'nav.advanced', icon: 'developer_mode' },
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
        {/* The design's rail sec-title (12-settings.html:149). A plain div,
            not a heading — the Header banner already carries the page h2 and
            each pane owns its own group headings. Desktop-only: the mobile
            rail is a horizontal tab strip where the label is noise. */}
        <div className="hidden md:block px-md pb-sm font-title-md text-on-surface font-bold tracking-wide">
          {intl.formatMessage({ id: 'nav.settings' })}
        </div>
        {sections.map((s) => (
          <NavLink
            key={s.to}
            to={s.to}
            className={({ isActive }) =>
              cn(
                'flex items-center gap-sm whitespace-nowrap rounded-lg px-md py-sm font-label-md transition-colors cursor-pointer',
                isActive
                  ? 'bg-primary-container text-on-primary-container font-medium'
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
        <div className="max-w-narrow mx-auto px-lg py-xl animate-in fade-in duration-(--duration-slower) pb-xl">
          <Outlet />
        </div>
      </div>
    </div>
  );
}
