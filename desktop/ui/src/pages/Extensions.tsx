import { useState } from "react";
import { Outlet, useLocation, useNavigate, NavLink } from "react-router-dom";
import { useIntl } from "react-intl";
import { Input } from "@/components/ui/input";
import { Button } from "@/components/ui/button";
import { DropdownMenu, type DropdownMenuItem } from "@/components/ui/dropdown-menu";
import { usePendingSkillCandidates } from "@/hooks/usePendingSkillCandidates";

/* 2026-09 review — the Extensions hub is rebuilt on the Codex/ZCode
 * marketplace pattern. The old seven-tab bar (精选 / MCP 服务器 / 技能 /
 * 我的 Agent / 数据源 / 插件 / 已安装) forced novices to learn the extension
 * taxonomy before they could install anything. Now:
 *   - three primary destinations: 扩展市场 (browse + install, the default),
 *     已安装 (everything installed, grouped by type), and 待处理 (IA X1:
 *     skill proposals awaiting review — the single skill-review surface,
 *     评审裁决 #2 — badge = pending count; later MCP/install errors);
 *   - the per-type management surfaces (MCP/Skills/Agents/DataSources/
 *     Plugins) collapse into one 管理 dropdown for power users — the routes
 *     are unchanged, only the chrome is simplified. */

const primaryTabs = [
  { to: '/extensions/featured', icon: 'auto_awesome', labelKey: 'extensions.featured' },
  { to: '/extensions/installed', icon: 'download', labelKey: 'extensions.installed' },
  { to: '/extensions/pending', icon: 'pending_actions', labelKey: 'extensions.pending', badge: true },
] as const

const manageEntries = [
  { to: '/extensions/mcp-servers', icon: 'cloud', labelKey: 'extensions.mcpServers' },
  { to: '/extensions/skills', icon: 'extension', labelKey: 'extensions.skills' },
  { to: '/extensions/agents', icon: 'smart_toy', labelKey: 'extensions.myAgents' },
  { to: '/extensions/datasources', icon: 'database', labelKey: 'extensions.dataSources' },
  { to: '/extensions/plugins', icon: 'workspaces', labelKey: 'extensions.plugins' },
] as const

export default function Extensions() {
  const location = useLocation();
  const navigate = useNavigate();
  const intl = useIntl();
  const t = (id: string) => intl.formatMessage({ id });
  const path = location.pathname;
  const [search, setSearch] = useState("");
  const [manageOpen, setManageOpen] = useState(false);
  // IA X1: 待处理 badge — same source of truth as the header bell and the
  // degraded toast (pending skill candidates; MCP errors join later).
  const { candidates } = usePendingSkillCandidates();
  const pendingCount = candidates.length;

  let searchPlaceholderKey = "extensions.search.placeholder";

  if (path.includes('agents')) {
    searchPlaceholderKey = "extensions.search.agents";
  } else if (path.includes('datasources')) {
    searchPlaceholderKey = "extensions.search.datasources";
  }

  const manageItems: DropdownMenuItem[] = manageEntries.map(entry => ({
    id: entry.to,
    label: t(entry.labelKey),
    icon: entry.icon,
    onSelect: () => { setManageOpen(false); navigate(entry.to) },
  }));
  const manageActive = manageEntries.some(e => path.includes(e.to.split('/').pop()!));

  return (
    <div className="flex-1 flex flex-col h-full bg-surface pb-[32px]">
      {/* ZCode marketplace header: title + one-line subtitle explain the page
          in plain words; search and the 管理 menu sit on the same row. On
          narrow widths the rows stack so neither squeezes the other. The
          shared search input is owned here and piped to the active tab via
          outlet context. */}
      <div className="flex flex-col gap-sm w-full px-lg py-sm border-b border-outline-variant/20 bg-surface/80 backdrop-blur-md sticky top-0 z-subheader">
        {/* Batch E1 (ZCode 市场大页形态): the hub carries its own H1 — the
            marketplace is a destination, not a settings subpage. */}
        <h1 className="font-headline-lg text-[24px] font-bold text-on-surface leading-tight">{t('extensions.hub.title')}</h1>
        <div className="flex items-center justify-between gap-md flex-wrap min-w-0">
          <p className="font-body-sm text-on-surface-variant truncate">{t('extensions.hub.subtitle')}</p>
          <div className="flex items-center bg-surface-container-lowest/50 rounded-full px-md py-xs border border-outline-variant/30 w-full max-w-[360px] focus-within:border-primary/40 focus-within:ring-2 focus-within:ring-primary/20 transition-colors shrink-0">
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
        <div className="flex items-center gap-xs flex-wrap min-w-0">
          <nav aria-label={t('extensions.tabs.aria')} className="flex items-center gap-xs flex-wrap min-w-0">
            {primaryTabs.map(tab => {
              // IA X1: the 待处理 tab's accessible name carries the badge
              // count (a bare chip number would read as "Pending 3").
              const badged = 'badge' in tab && tab.badge && pendingCount > 0
              return (
              <NavLink
                key={tab.to}
                to={tab.to}
                aria-label={badged ? intl.formatMessage({ id: 'extensions.pending.tabBadge.aria' }, { count: pendingCount }) : undefined}
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
                    <span>{t(tab.labelKey)}</span>
                    {badged && (
                      <span
                        aria-hidden="true"
                        title={intl.formatMessage({ id: 'extensions.pending.tabBadge.aria' }, { count: pendingCount })}
                        className="ml-1 px-[7px] min-w-[18px] h-[18px] inline-flex items-center justify-center rounded-full bg-primary text-on-primary font-label-sm text-[11px] font-bold leading-none"
                      >
                        {pendingCount}
                      </span>
                    )}
                  </>
                )}
              </NavLink>
              )
            })}
          </nav>
          <span className="relative">
            <Button
              variant="ghost"
              size="sm"
              aria-haspopup="menu"
              aria-expanded={manageOpen}
              className={`flex items-center gap-xs px-md py-xs rounded-xl font-label-md text-label-md whitespace-nowrap transition-all ${
                manageActive
                  ? 'bg-primary/15 text-primary font-bold'
                  : 'text-on-surface-variant hover:text-primary hover:bg-surface-container-low'
              }`}
              onClick={() => setManageOpen(v => !v)}
            >
              <span className="material-symbols-outlined text-[18px]" aria-hidden="true">tune</span>
              <span>{t('extensions.manage')}</span>
              <span className="material-symbols-outlined text-[16px] transition-transform" style={{ transform: manageOpen ? 'rotate(180deg)' : undefined }} aria-hidden="true">expand_more</span>
            </Button>
            {manageOpen && (
              <DropdownMenu
                open
                onClose={() => setManageOpen(false)}
                items={manageItems}
                align="start"
                className="w-48 min-w-0"
                ariaLabel={t('extensions.manage')}
              />
            )}
          </span>
        </div>
      </div>

      {/* Content Area */}
      <div className="flex-1 overflow-y-auto">
         <Outlet context={{ search }} />
      </div>
    </div>
  );
}
