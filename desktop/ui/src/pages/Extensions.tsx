import { useEffect, useState } from "react";
import { Outlet, useLocation, useNavigate, NavLink } from "react-router-dom";
import { useIntl } from "react-intl";
import { Input } from "@/components/ui/input";
import { DropdownMenu, type DropdownMenuItem } from "@/components/ui/dropdown-menu";
import { usePendingSkillCandidates } from "@/hooks/usePendingSkillCandidates";
import { listInstalledAddons } from "@/lib/tauri-api";

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

// G4a (设计 07): the primary tabs render as text underline-tabs — the icon
// field G1's chip strip carried is dropped (the design's tabs are text +
// count only).
const primaryTabs = [
  { to: '/extensions/featured', labelKey: 'extensions.featured' },
  { to: '/extensions/installed', labelKey: 'extensions.installed' },
  { to: '/extensions/pending', labelKey: 'extensions.pending', badge: true },
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
  // G4a (design 07 下划线 tab 计数): the 已安装 tab's count pill — same
  // aggregator Featured matches installs against. A failed/absent read (demo
  // mode) renders no number rather than a fake zero.
  const [installedCount, setInstalledCount] = useState<number | null>(null);
  // IA X1: 待处理 badge — same source of truth as the header bell and the
  // degraded toast (pending skill candidates; MCP errors join later).
  const { candidates } = usePendingSkillCandidates();
  const pendingCount = candidates.length;

  useEffect(() => {
    let cancelled = false;
    listInstalledAddons()
      .then((rows) => { if (!cancelled) setInstalledCount(Array.isArray(rows) ? rows.length : null) })
      .catch(() => { if (!cancelled) setInstalledCount(null) });
    return () => { cancelled = true };
  }, []);

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
      {/* G1: sticky page toolbar = persistent chrome → glass-surface (was a
          hand-rolled bg-surface/80+backdrop-blur-md). */}
      <div className="glass-surface flex flex-col gap-sm w-full px-lg py-sm sticky top-0 z-subheader">
        {/* Batch E1 (ZCode 市场大页形态): the hub carries its own H1 — the
            marketplace is a destination, not a settings subpage. */}
        <h1 className="font-headline-lg text-headline-md font-bold text-on-surface leading-tight">{t('extensions.hub.title')}</h1>
        <div className="flex items-center justify-between gap-md flex-wrap min-w-0">
          {/* M1 (design-parity R2 §3.2, 07-connectors.html:141): the topbar
              crumb 「目录内条目均经安全扫描」 — the security sentence rides
              next to the capability subtitle as its own line-level crumb. */}
          <div className="flex items-center gap-md min-w-0">
            <p className="font-body-sm text-on-surface-variant truncate">{t('extensions.hub.subtitle')}</p>
            <span
              className="inline-flex items-center gap-xs shrink-0 font-label-sm text-on-surface-variant whitespace-nowrap"
              title={t('extensions.hub.securityCrumb')}
            >
              <span className="material-symbols-outlined icon-sm text-success" aria-hidden="true">verified_user</span>
              {t('extensions.hub.securityCrumb')}
            </span>
          </div>
          <div className="flex items-center bg-surface-container-lowest/50 rounded-full px-md py-xs border border-outline-variant/30 w-full max-w-[360px] focus-within:border-primary/40 focus-within:ring-2 focus-within:ring-primary/20 transition-colors shrink-0">
            <span className="material-symbols-outlined text-on-surface-variant mr-sm icon-md">search</span>
            <Input
              className="bg-transparent border-none outline-none focus:ring-0 text-label-md font-label-md w-full"
              placeholder={t(searchPlaceholderKey)}
              type="text"
              value={search}
              onChange={(e) => setSearch(e.target.value)}
            />
          </div>
        </div>
        <div className="flex items-end justify-between gap-md flex-wrap min-w-0">
          {/* G4a (设计 07): the primary destinations are UNDERLINE tabs —
              text tab + the 2px aurora gradient underline (.aurora-tab) +
              a small grey count pill — the upgraded form of G1's chip strip.
              The bottom hairline seats the underline (same grammar as the
              Tasks tab row). */}
          <nav
            aria-label={t('extensions.tabs.aria')}
            className="flex items-center gap-md border-b border-outline-variant/30 min-w-0"
          >
            {primaryTabs.map(tab => {
              // IA X1: the 待处理 tab's accessible name carries the badge
              // count (a bare pill number would read as "Pending 3").
              const badged = 'badge' in tab && tab.badge && pendingCount > 0
              // G4a count pill: Installed (live aggregator read) / Pending
              // (skill candidates). No data → no pill, never a fake zero.
              const count = tab.to.endsWith('/installed')
                ? installedCount
                : 'badge' in tab && tab.badge
                  ? pendingCount
                  : null
              return (
              <NavLink
                key={tab.to}
                to={tab.to}
                aria-label={badged ? intl.formatMessage({ id: 'extensions.pending.tabBadge.aria' }, { count: pendingCount }) : undefined}
                className={({ isActive }) =>
                  `relative flex items-center gap-xs px-md py-sm -mb-px text-label-md cursor-pointer whitespace-nowrap transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary/40 focus-visible:ring-offset-1 focus-visible:ring-offset-surface ${
                    isActive
                      ? 'text-on-surface font-semibold aurora-tab'
                      : 'text-on-surface-variant hover:text-on-surface'
                  }`
                }
              >
              <>
                <span>{t(tab.labelKey)}</span>
                {count != null && count > 0 && (
                  <span
                    aria-hidden="true"
                    title={badged ? intl.formatMessage({ id: 'extensions.pending.tabBadge.aria' }, { count: pendingCount }) : undefined}
                    data-testid={`extensions-tab-count-${tab.to.split('/').pop()}`}
                    className="px-[6px] min-w-[18px] h-[18px] inline-flex items-center justify-center rounded-full bg-on-surface-variant/10 text-on-surface-variant text-label-2xs font-bold tabular-nums leading-none"
                  >
                    {count}
                  </span>
                )}
              </>
              </NavLink>
              )
            })}
          </nav>
          <span className="relative">
            {/* X4 管理瘦身: the five type-specific managers are labelled
                「高级管理」 (advanced) — novices work from the three primary
                tabs + Installed jump chips; power users keep every route.
                G1-B (设计 07): the trigger stays a quiet ghost/chip — the
                solid violet pill used to out-shout the primary tabs; while a
                manager route is active it reads violet dim, never solid. */}
            <button
              type="button"
              aria-haspopup="menu"
              aria-expanded={manageOpen}
              className={`chip cursor-pointer focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary/40 ${
                manageActive
                  ? 'border border-primary/40 bg-primary-container text-on-primary-container font-semibold'
                  : 'border border-outline-variant/40 bg-surface-container/30 text-on-surface-variant hover:bg-surface-container/60 hover:text-on-surface'
              }`}
              onClick={() => setManageOpen(v => !v)}
            >
              <span className="material-symbols-outlined" style={{ fontSize: 13 }} aria-hidden="true">tune</span>
              <span>{t('extensions.manage')}</span>
              <span className="material-symbols-outlined transition-transform" style={{ fontSize: 12, opacity: 0.6, transform: manageOpen ? 'rotate(180deg)' : undefined }} aria-hidden="true">expand_more</span>
            </button>
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
      {/* tabIndex: keyboard-scrollable region (axe scrollable-region-
          focusable) — the Pending tab's content overflows this container,
          and without focus keyboard users can never reach the overflow. */}
      <div tabIndex={0} className="flex-1 overflow-y-auto">
         <Outlet context={{ search }} />
         {/* M1 (design-parity R2 §3.2, 07-connectors.html:322-327): the
             page-bottom security banner. Honest wording — the scan is the
             install-time advisory pass the card-level badges already state
             (never an overclaiming「签名校验」, which the batch-2 honesty
             fix removed from the card foot), and the credential sentence
             matches the Welcome footer's 「密钥永不出机」 narrative
             (welcome.privacy.keysLocal). The design's 安全白皮书 button is
             intentionally absent — there is no whitepaper to point at. */}
         <div className="px-lg pb-lg">
           <div
             data-testid="extensions-security-banner"
             className="mx-auto max-w-7xl flex items-center gap-sm px-md py-sm rounded-xl border border-outline-variant/30 bg-surface-container-low"
           >
             <span className="material-symbols-outlined icon-md text-success shrink-0" aria-hidden="true">verified_user</span>
             <p className="font-label-sm text-on-surface-variant min-w-0">{t('extensions.hub.securityBanner')}</p>
           </div>
         </div>
      </div>
    </div>
  );
}
