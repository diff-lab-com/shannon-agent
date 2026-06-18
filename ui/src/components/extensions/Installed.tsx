import { useEffect, useState } from "react";
import { useOutletContext } from "react-router-dom";
import { useIntl } from "react-intl";
import { listInstalledAddons } from "@/lib/tauri-api";
import type { InstalledAddonSummary, AddonKind } from "@/types";

/**
 * Installed tab — P1's only fully-wired view.
 *
 * Calls `list_installed_addons` Tauri command which aggregates:
 * - MCP servers from `~/.shannon/settings.json` and `.mcp.json`
 * - Skills from `~/.shannon/skills/` and `.claude/commands/`
 * - Agents from `~/.shannon/agents/` and `.claude/agents/`
 *
 * No write path in P1 — uninstall/remove still happens on the Skills tab
 * (for skills) and Settings page (for MCP servers).
 */
export default function Installed() {
  const intl = useIntl();
  const t = (id: string) => intl.formatMessage({ id });
  const { search } = useOutletContext<{ search: string }>();
  const [addons, setAddons] = useState<InstalledAddonSummary[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

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
      <div className="p-lg max-w-4xl mx-auto">
        <div className="text-center py-3xl text-on-surface-variant">
          <span className="material-symbols-outlined animate-spin text-[32px] mb-md">progress_activity</span>
          <p className="text-body-md">{t('extensions.installed.scanning')}</p>
        </div>
      </div>
    );
  }

  if (error) {
    return (
      <div className="p-lg max-w-4xl mx-auto">
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
      <div className="p-lg max-w-4xl mx-auto">
        <div className="text-center py-3xl">
          <div className="inline-flex items-center justify-center w-16 h-16 rounded-full bg-surface-container-low mb-md">
            <span className="material-symbols-outlined text-on-surface-variant text-[32px]">download</span>
          </div>
          <h2 className="text-headline-md font-bold text-on-surface mb-sm">
            {t('extensions.installed.nothingInstalled')}
          </h2>
          <p className="text-body-md text-on-surface-variant max-w-md mx-auto">
            {t('extensions.installed.nothingDesc')}
          </p>
        </div>
      </div>
    );
  }

  const populatedKinds = KIND_ORDER.filter((k) => grouped[k].length > 0);

  return (
    <div className="p-lg max-w-4xl mx-auto">
      <header className="mb-lg">
        <h1 className="text-headline-sm font-bold text-on-surface">
          {t('extensions.installed.title')}
        </h1>
        <p className="text-label-sm text-on-surface-variant">
          {intl.formatMessage({ id: 'extensions.installed.count' }, {
            entries: filtered.length,
            categories: populatedKinds.length,
          })}
        </p>
      </header>

      <div className="space-y-xl">
        {populatedKinds.map((kind) => {
          const rows = grouped[kind];
          return (
            <section key={kind}>
              <h2 className="text-label-lg font-bold text-on-surface-variant uppercase tracking-wide mb-sm">
                {kindLabel(intl, kind)}{' · '}{rows.length}
              </h2>
              <div className="border border-outline-variant/30 rounded-2xl overflow-hidden bg-surface-container-lowest/50">
                {rows.map((row, i) => (
                  <InstalledRow key={row.id} row={row} isLast={i === rows.length - 1} />
                ))}
              </div>
            </section>
          );
        })}
      </div>
    </div>
  );
}

function InstalledRow({ row, isLast }: { row: InstalledAddonSummary; isLast: boolean }) {
  const intl = useIntl();
  const t = (id: string) => intl.formatMessage({ id });
  return (
    <div className={`flex items-start gap-md px-md py-sm ${isLast ? "" : "border-b border-outline-variant/15"}`}>
      <div className={`w-9 h-9 rounded-lg flex items-center justify-center shrink-0 ${row.enabled ? "bg-primary/10" : "bg-surface-container-low"}`}>
        <span className={`material-symbols-outlined text-[20px] ${row.enabled ? "text-primary" : "text-on-surface-variant"}`}>
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
        {row.install_path && (
          <p className="text-label-xs text-on-surface-variant font-mono truncate mt-[2px]">
            {row.install_path}
          </p>
        )}
        {row.installed_at && (
          <p className="text-label-xs text-outline mt-[2px]">
            {intl.formatMessage({ id: 'extensions.installed.installedAt' }, { date: formatDate(row.installed_at) })}
          </p>
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
    out[row.kind].push(row);
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
