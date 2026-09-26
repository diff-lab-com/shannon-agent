import { useEffect, useState } from "react";
import LoadingState from "@/components/ui/loading-state";
import { useNavigate, useOutletContext } from "react-router-dom";
import { useIntl } from "react-intl";
import { getExtensionStats, listInstalledAddons } from "@/lib/tauri-api";
import type { InstalledAddonSummary, AddonKind, ExtensionStats, ExtensionToolStatRow } from "@/types";
import EmptyState from "@/components/ui/empty-state";
import { cn } from "@/lib/utils";

/** Stats look-back window (days) — the REQUEST default. The row subtext
 *  displays the server-echoed `ExtensionStats.days` (falling back to this
 *  constant only while stats are absent), so the wording never claims a
 *  window the backend did not use. */
const STATS_WINDOW_DAYS = 30;

/**
 * Installed tab — P1's only fully-wired view.
 *
 * Calls `list_installed_addons` Tauri command which aggregates:
 * - MCP servers from `~/.shannon/settings.json` and `.mcp.json`
 * - Skills from `~/.shannon/skills/` and `.claude/commands/`
 * - Agents from `~/.shannon/agents/` and `.claude/agents/`
 *
 * Each matching row also carries an X7 usage subtext (calls + token cost
 * within {@link STATS_WINDOW_DAYS}, from `get_extension_stats`). Rows with
 * no stats render exactly as before.
 *
 * No write path in P1 — uninstall/remove still happens on the Skills tab
 * (for skills) and Settings page (for MCP servers).
 */
export default function Installed() {
  const intl = useIntl();
  const t = (id: string) => intl.formatMessage({ id });
  const navigate = useNavigate();
  const { search } = useOutletContext<{ search: string }>();
  const [addons, setAddons] = useState<InstalledAddonSummary[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [stats, setStats] = useState<ExtensionStats | null>(null);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    listInstalledAddons()
      .then((rows) => {
        if (!cancelled) {
          setAddons(rows);
          setError(null);
        }
      })
      .catch((err) => {
        if (!cancelled) setError(String(err));
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, []);

  // X7 stats are advisory: fetched once per mount, and a failure simply
  // leaves the rows without their usage subtext.
  useEffect(() => {
    let cancelled = false;
    getExtensionStats(STATS_WINDOW_DAYS)
      .then((s) => {
        if (!cancelled) setStats(s);
      })
      .catch(() => {
        if (!cancelled) setStats(null);
      });
    return () => {
      cancelled = true;
    };
  }, []);

  // Auto-refresh when an extension is installed from the marketplace
  useEffect(() => {
    const handleInstalledEvent = () => {
      listInstalledAddons()
        .then((rows) => {
          setAddons(rows);
          setError(null);
        })
        .catch((err) => {
          setError(String(err));
        });
    };

    window.addEventListener("shannon:extension-installed", handleInstalledEvent);
    return () => {
      window.removeEventListener("shannon:extension-installed", handleInstalledEvent);
    };
  }, []);

  const filtered = search
    ? addons.filter(
        (a) =>
          a.name.toLowerCase().includes(search.toLowerCase()) ||
          a.id.toLowerCase().includes(search.toLowerCase())
      )
    : addons;

  const grouped = groupByKind(filtered);

  if (loading) {
    return (
      <div className="p-lg max-w-7xl mx-auto">
        <LoadingState size="lg" label={t('extensions.installed.scanning')} />
      </div>
    );
  }

  if (error) {
    return (
      <div className="p-lg max-w-7xl mx-auto">
        <div className="border border-error/30 rounded-2xl p-lg bg-error-container/20">
          <div className="flex items-start gap-md">
            <span className="material-symbols-outlined text-error text-[24px]">error</span>
            <div>
              <h3 className="font-bold text-error mb-xs">{t('extensions.installed.loadFailed')}</h3>
              <p className="text-label-sm text-on-surface-variant font-mono">{error}</p>
            </div>
          </div>
        </div>
      </div>
    );
  }

  if (filtered.length === 0) {
    return (
      <div className="p-lg max-w-7xl mx-auto">
        <EmptyState
          icon="download"
          title={t('extensions.installed.nothingInstalled')}
          description={t('extensions.installed.nothingDesc')}
          action={{ label: t('extensions.installed.cta'), onClick: () => navigate('/extensions/skills') }}
        />
      </div>
    );
  }

  const populatedKinds = KIND_ORDER.filter((k) => grouped[k].length > 0);

  return (
    <div className="p-lg max-w-7xl mx-auto">
      <p className="text-label-sm text-on-surface-variant mb-lg">
        {intl.formatMessage({ id: 'extensions.installed.count' }, {
          entries: filtered.length,
          categories: populatedKinds.length,
        })}
      </p>

      {/* X4 锚点分区导航 — the five type surfaces live behind the 管理
          (advanced) menu, so the Installed tab carries its own jump chips.
          Click = scroll to the group; no extra page chrome. */}
      {populatedKinds.length > 1 && (
        <nav
          aria-label={t('extensions.installed.jumpNav.aria')}
          data-testid="installed-jump-nav"
          className="flex flex-wrap gap-xs mb-lg"
        >
          {populatedKinds.map((kind) => (
            <button
              key={kind}
              type="button"
              data-testid={`installed-jump-${kind}`}
              onClick={() =>
                document
                  .getElementById(`installed-section-${kind}`)
                  ?.scrollIntoView({ behavior: 'smooth', block: 'start' })
              }
              className="inline-flex items-center gap-xs px-sm py-xs rounded-full border border-outline-variant/30 bg-surface-container-low text-label-sm text-on-surface-variant hover:border-primary/40 hover:text-primary transition-colors cursor-pointer focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary/40"
            >
              <span className="material-symbols-outlined text-[14px]" aria-hidden="true">
                {KIND_ICONS[kind]}
              </span>
              {kindLabel(intl, kind)}
              <span className="font-bold">{grouped[kind].length}</span>
            </button>
          ))}
        </nav>
      )}

      <div className="space-y-xl">
        {populatedKinds.map((kind) => {
          const rows = grouped[kind];
          return (
            <section
              key={kind}
              id={`installed-section-${kind}`}
              data-testid={`installed-section-${kind}`}
              className="scroll-mt-24"
            >
              <h2 className="text-label-lg font-bold text-on-surface-variant uppercase tracking-wide mb-sm">
                {kindLabel(intl, kind)}{' · '}{rows.length}
              </h2>
              <div className="border border-outline-variant/30 rounded-2xl overflow-hidden bg-surface-container-lowest/50">
                {rows.map((row, i) => (
                  <InstalledRow
                    key={row.id}
                    row={row}
                    isLast={i === rows.length - 1}
                    stat={statFor(stats, row)}
                    days={stats?.days ?? STATS_WINDOW_DAYS}
                  />
                ))}
              </div>
            </section>
          );
        })}
      </div>
    </div>
  );
}

const KIND_TO_PATH: Record<AddonKind, string> = {
  mcp: '/extensions/mcp-servers',
  skill: '/extensions/skills',
  agent: '/extensions/agents',
  data_source: '/extensions/datasources',
  plugin: '/extensions/plugins',
};

const KIND_TO_MANAGE_TAB: Record<AddonKind, string> = {
  mcp: 'extensions.mcpServers',
  skill: 'extensions.skills',
  agent: 'extensions.myAgents',
  data_source: 'extensions.dataSources',
  plugin: 'extensions.plugins',
};

/**
 * X7: the usage stats matching one Installed row, if any. Skill rows match
 * by skill id (the `skill_` engine prefix is stripped server-side), MCP
 * rows by server name; every other kind has no engine tool name to match.
 */
function statFor(
  stats: ExtensionStats | null,
  row: InstalledAddonSummary
): ExtensionToolStatRow | undefined {
  if (!stats) return undefined;
  if (row.kind === 'skill') return stats.skills.find((s) => s.name === row.name);
  if (row.kind === 'mcp') {
    const server = stats.mcpServers.find((s) => s.server === row.name);
    return server ? { name: server.server, calls: server.calls, totalTokens: server.totalTokens } : undefined;
  }
  return undefined;
}

function InstalledRow({
  row,
  isLast,
  stat,
  days,
}: {
  row: InstalledAddonSummary
  isLast: boolean
  /** Row-matched usage stats; `undefined` renders no subtext. */
  stat?: ExtensionToolStatRow
  days: number
}) {
  const intl = useIntl();
  const t = (id: string) => intl.formatMessage({ id });
  const navigate = useNavigate();
  return (
    <div className={cn("flex items-start gap-md px-md py-sm", isLast ? "" : "border-b border-outline-variant/15")}>
      <div className={cn("w-9 h-9 rounded-lg flex items-center justify-center shrink-0", row.enabled ? "bg-primary/10" : "bg-surface-container-low")}>
        <span className={cn("material-symbols-outlined icon-md", row.enabled ? "text-primary" : "text-on-surface-variant")}>
          {KIND_ICONS[row.kind]}
        </span>
      </div>
      <div className="flex-1 min-w-0">
        <div className="flex items-center gap-sm">
          <h3 className="font-bold text-label-md text-on-surface truncate">{row.name}</h3>
          {row.version && (
            <span className="text-label-xs px-xs py-[1px] rounded bg-surface-container-low text-on-surface-variant font-mono">
              {row.version}
            </span>
          )}
          {!row.enabled && (
            <span className="text-label-xs px-xs py-[1px] rounded bg-warning-container/50 text-on-warning-container font-bold">
              {t('extensions.installed.disabled')}
            </span>
          )}
        </div>
        {stat && stat.calls > 0 && (
          <p className="text-label-xs text-on-surface-variant mt-[2px]" data-testid="installed-row-stats">
            {intl.formatMessage({ id: 'extensions.installed.statsCalls' }, { calls: stat.calls, days })}
            {stat.totalTokens > 0 &&
              intl.formatMessage({ id: 'extensions.installed.statsTokens' }, { tokens: stat.totalTokens })}
          </p>
        )}
        {row.install_path && (
          <p className="text-label-xs text-on-surface-variant font-mono truncate mt-[2px]">
            {row.install_path}
          </p>
        )}
        {row.installed_at && (
          <p className="text-label-xs text-on-surface-variant mt-[2px]">
            {intl.formatMessage({ id: 'extensions.installed.installedAt' }, { date: formatDate(row.installed_at) })}
          </p>
        )}
        {/* Disabled entries have no in-row toggle (no write Tauri command yet),
            but they must never be a dead-end: jump to the matching 管理 tab. */}
        {!row.enabled && (
          <button
            type="button"
            onClick={() => navigate(KIND_TO_PATH[row.kind])}
            className="mt-xs inline-flex items-center gap-0.5 text-label-xs text-primary hover:underline focus-visible:outline focus-visible:outline-2 focus-visible:outline-primary/40 rounded px-0.5 -mx-0.5"
            title={intl.formatMessage({ id: 'extensions.installed.disabledCtaHint' }, { tab: t(KIND_TO_MANAGE_TAB[row.kind]) })}
          >
            <span className="material-symbols-outlined text-[14px]" aria-hidden="true">arrow_forward</span>
            {t('extensions.installed.disabledCta')} {t(KIND_TO_MANAGE_TAB[row.kind])} →
          </button>
        )}
      </div>
    </div>
  );
}

const KIND_ORDER: AddonKind[] = ["mcp", "skill", "agent", "data_source", "plugin"];

const KIND_LABEL_KEYS: Record<AddonKind, string> = {
  mcp: "extensions.installed.mcpServers",
  skill: "extensions.installed.skills",
  agent: "extensions.installed.agents",
  data_source: "extensions.installed.dataSources",
  plugin: "extensions.installed.plugins",
};

function kindLabel(intl: ReturnType<typeof useIntl>, kind: AddonKind): string {
  return intl.formatMessage({ id: KIND_LABEL_KEYS[kind] });
}

const KIND_ICONS: Record<AddonKind, string> = {
  mcp: "cloud",
  skill: "extension",
  agent: "smart_toy",
  data_source: "database",
  plugin: "workspaces",
};

function groupByKind(rows: InstalledAddonSummary[]): Record<AddonKind, InstalledAddonSummary[]> {
  const out: Record<AddonKind, InstalledAddonSummary[]> = {
    mcp: [],
    skill: [],
    agent: [],
    data_source: [],
    plugin: [],
  };
  for (const row of rows) {
    // Unknown kinds must not crash the page (defensive against a newer
    // backend emitting a kind this build doesn't know).
    const bucket = out[row.kind];
    if (bucket) bucket.push(row);
    else console.warn("[installed] unknown addon kind:", row.kind);
  }
  return out;
}

function formatDate(iso: string): string {
  try {
    const d = new Date(iso);
    return d.toLocaleDateString(undefined, { year: "numeric", month: "short", day: "numeric" });
  } catch {
    return iso;
  }
}
