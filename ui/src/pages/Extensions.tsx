import { useState } from "react";
import { Outlet, useLocation, NavLink } from "react-router-dom";
import { useIntl } from "react-intl";
import { Input } from "@/components/ui/input";

const subTabs = [
  { to: '/extensions/featured', icon: 'auto_awesome', labelKey: 'extensions.featured' },
  { to: '/extensions/mcp-servers', icon: 'cloud', labelKey: 'extensions.mcpServers' },
  { to: '/extensions/skills', icon: 'extension', labelKey: 'extensions.skills' },
  { to: '/extensions/agents', icon: 'smart_toy', labelKey: 'extensions.myAgents' },
  { to: '/extensions/datasources', icon: 'database', labelKey: 'extensions.dataSources' },
  { to: '/extensions/plugins', icon: 'workspaces', labelKey: 'extensions.plugins' },
  { to: '/extensions/installed', icon: 'download', labelKey: 'extensions.installed' },
] as const

export default function Extensions() {
  const location = useLocation();
  const intl = useIntl();
  const t = (id: string) => intl.formatMessage({ id });
  const path = location.pathname;
  const [search, setSearch] = useState("");

  let searchPlaceholderKey = "extensions.search.placeholder";

  if (path.includes('agents')) {
    searchPlaceholderKey = "extensions.search.agents";
  } else if (path.includes('datasources')) {
    searchPlaceholderKey = "extensions.search.datasources";
  }

  return (
    <div className="flex-1 flex flex-col h-full bg-surface pb-[32px]">
      {/* Top Bar with tabs + search + CTA.
          On narrow widths the tabs row and the search/CTA row stack
          vertically so neither squeezes the other. The shared search input
          is owned here and piped to the active tab via outlet context. */}
      <div className="flex flex-col gap-sm w-full px-lg py-sm border-b border-outline-variant/20 bg-surface/80 backdrop-blur-md sticky top-0 z-30">
        <nav aria-label={t('extensions.tabs.aria')} className="flex items-center gap-xs flex-wrap">
          {subTabs.map(tab => (
            <NavLink
              key={tab.to}
              to={tab.to}
              className={({ isActive }) =>
                `flex items-center gap-xs px-md py-xs rounded-xl font-label-md text-label-md transition-all whitespace-nowrap focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary/40 focus-visible:ring-offset-1 focus-visible:ring-offset-surface ${
                  isActive
                    ? 'bg-primary/15 text-primary font-bold'
                    : 'text-on-surface-variant hover:text-primary hover:bg-surface-container-low'
                }`
              }
            >
              {({ isActive }) => (
                <>
                  <span
                    className="material-symbols-outlined text-[18px]"
                    style={{ fontVariationSettings: isActive ? "'FILL' 1" : undefined }}
                    aria-hidden="true"
                  >
                    {tab.icon}
                  </span>
                  <span className="hidden sm:inline">{t(tab.labelKey)}</span>
                </>
              )}
            </NavLink>
          ))}
        </nav>
        <div className="flex items-center justify-end gap-md">
          <div className="flex items-center bg-surface-container-lowest/50 rounded-full px-md py-xs border border-outline-variant/30 w-full max-w-[360px] focus-within:border-primary/40 focus-within:ring-2 focus-within:ring-primary/20 transition-colors">
            <span className="material-symbols-outlined text-outline mr-sm text-[18px]">search</span>
            <Input
              className="bg-transparent border-none outline-none focus:ring-0 text-label-md font-label-md w-full"
              placeholder={t(searchPlaceholderKey)}
              type="text"
              value={search}
              onChange={(e) => setSearch(e.target.value)}
            />
          </div>
        </div>
      </div>

      {/* Content Area */}
      <div className="flex-1 overflow-y-auto">
         <Outlet context={{ search }} />
      </div>
    </div>
  );
}
