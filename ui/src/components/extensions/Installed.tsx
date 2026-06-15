import { useEffect, useState } from "react";
import { useOutletContext } from "react-router-dom";
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
          <p className="text-body-md">Scanning local configs…</p>
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
              <h3 className="font-bold text-error mb-xs">Failed to load installed addons</h3>
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
            Nothing installed yet
          </h2>
          <p className="text-body-md text-on-surface-variant max-w-md mx-auto">
            Browse the Featured, MCP Servers, Skills, and Plugins tabs to add
            your first extension.
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
          Installed Extensions
        </h1>
        <p className="text-label-sm text-on-surface-variant">
          {filtered.length} {filtered.length === 1 ? "entry" : "entries"} across {populatedKinds.length} {populatedKinds.length === 1 ? "category" : "categories"}
        </p>
      </header>

      <div className="space-y-xl">
        {populatedKinds.map((kind) => {
          const rows = grouped[kind];
          return (
            <section key={kind}>
              <h2 className="text-label-lg font-bold text-on-surface-variant uppercase tracking-wide mb-sm">
                {KIND_LABELS[kind]}{' · '}{rows.length}
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
              Disabled
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
            installed {formatDate(row.installed_at)}
          </p>
        )}
      </div>
    </div>
  );
}

const KIND_ORDER: AddonKind[] = ["mcp", "skill", "agent", "data_source", "plugin"];

const KIND_LABELS: Record<AddonKind, string> = {
  mcp: "MCP Servers",
  skill: "Skills",
  agent: "Agents",
  data_source: "Data Sources",
  plugin: "Plugins",
};

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
