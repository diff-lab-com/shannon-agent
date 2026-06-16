import { useState } from "react";
import { Outlet, useLocation, useNavigate, NavLink } from "react-router-dom";
import { useIntl } from "react-intl";
import { Button } from "@/components/ui/button";
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
  const navigate = useNavigate();
  const intl = useIntl();
  const t = (id: string) => intl.formatMessage({ id });
  const path = location.pathname;
  const [search, setSearch] = useState("");

  let searchPlaceholderKey = "extensions.search.placeholder";
  let ctaTextKey = "";
  let ctaIcon = "";
  let ctaAction: () => void = () => {};

  if (path.includes('agents')) {
    searchPlaceholderKey = "extensions.search.agents";
    ctaTextKey = "extensions.createAgent";
    ctaIcon = "add";
    ctaAction = () => navigate('/extensions/agents');
  } else if (path.includes('datasources')) {
    searchPlaceholderKey = "extensions.search.datasources";
    ctaTextKey = "extensions.addSource";
    ctaIcon = "add_circle";
    ctaAction = () => navigate('/extensions/datasources');
  }

  return (
    <div className="flex-1 flex flex-col h-full bg-surface pb-[32px]">
      {/* Top Bar with tabs + search + CTA */}
      <div className="flex justify-between items-center w-full px-lg py-sm border-b border-outline-variant/20 bg-surface/80 backdrop-blur-md sticky top-0 z-30">
        <div className="flex items-center gap-lg">
          <nav aria-label={t('extensions.tabs.aria')} className="flex items-center gap-xs flex-wrap">
            {subTabs.map(tab => (
              <NavLink
                key={tab.to}
                to={tab.to}
                className={({ isActive }) =>
                  `flex items-center gap-xs px-md py-xs rounded-lg font-label-md text-label-md transition-all ${
                    isActive
                      ? 'bg-primary/10 text-primary font-bold'
                      : 'text-on-surface-variant hover:text-primary hover:bg-surface-container-low'
                  }`
                }
              >
                <span className="material-symbols-outlined text-[18px]">{tab.icon}</span>
                <span className="hidden sm:inline">{t(tab.labelKey)}</span>
              </NavLink>
            ))}
          </nav>
        </div>
        <div className="flex items-center gap-md shrink-0">
          <div className="hidden lg:flex items-center bg-surface-container-lowest/50 rounded-full px-md py-xs border border-outline-variant/30 max-w-[240px]">
            <span className="material-symbols-outlined text-outline mr-sm text-[18px]">search</span>
            <Input
              className="bg-transparent border-none outline-none focus:ring-0 text-label-md font-label-md w-full"
              placeholder={t(searchPlaceholderKey)}
              type="text"
              value={search}
              onChange={(e) => setSearch(e.target.value)}
            />
          </div>
          {ctaTextKey && (
            <Button onClick={ctaAction} className="bg-primary text-on-primary px-lg py-sm rounded-full font-bold text-label-md hover:bg-primary/90 flex items-center gap-1 cursor-pointer whitespace-nowrap">
              <span className="material-symbols-outlined text-[18px]">{ctaIcon}</span>
              {t(ctaTextKey)}
            </Button>
          )}
        </div>
      </div>

      {/* Content Area */}
      <div className="flex-1 overflow-y-auto">
         <Outlet context={{ search }} />
      </div>
    </div>
  );
}
