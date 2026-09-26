import { useCallback, useEffect, useMemo, useState } from "react";
import { FormattedMessage, useIntl } from "react-intl";
import { useOutletContext } from "react-router-dom";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { toast } from "sonner";
import * as api from "@/lib/tauri-api";
import type { CatalogUpstream, PluginInfo } from "@/lib/tauri-api";
import { CardSkeleton } from "@/components/SkeletonLoader";
import ErrorState from "@/components/ui/error-state";
import EmptyState from "@/components/ui/empty-state";
import LoadingState from "@/components/ui/loading-state";
import type { CatalogEntry, CatalogSource, PluginBundleSummary, TrustLevel } from "@/types";
import InstallDialog from "./InstallDialog";
import AddPluginGitDialog from "./AddPluginGitDialog";
import { Button } from "@/components/ui/button";
import { DropdownMenu, type DropdownMenuItem } from "@/components/ui/dropdown-menu";
import { Modal, ModalBody, ModalFooter } from "@/components/ui/modal";
import { Switch } from "@/components/ui/switch";
import { safeErrorMessage } from "@/lib/packageValidation";
import { cn } from "@/lib/utils";

type SortMode = "trust" | "stars" | "name" | "recent";
type TrustFilter = TrustLevel | "all";
type SourceFilter = CatalogSource["type"] | "all";
type PluginSource = PluginInfo["source"];

const TRUST_FILTER_ORDER: TrustFilter[] = ["all", "verified", "official", "community", "unknown"];
const SOURCE_FILTERS: SourceFilter[] = ["all", "git_hub_repo", "featured_vendor", "native", "mcp_registry", "custom"];

const TRUST_ICON: Record<TrustLevel, string> = {
  unknown: "help",
  community: "group",
  official: "verified_user",
  verified: "verified",
};

const TRUST_LABEL_KEY: Record<TrustLevel, string> = {
  unknown: "extensions.plugins.trustUnknown",
  community: "extensions.plugins.trustCommunity",
  official: "extensions.plugins.trustOfficial",
  verified: "extensions.plugins.trustVerified",
};

const TRUST_BADGE_CLASS: Record<TrustLevel, string> = {
  unknown: "bg-surface-container-high text-on-surface-variant",
  community: "bg-secondary/15 text-secondary",
  official: "bg-primary/15 text-primary",
  verified: "bg-tertiary/20 text-tertiary",
};

const TRUST_ORDER: Record<TrustLevel, number> = {
  verified: 0,
  official: 1,
  community: 2,
  unknown: 3,
};

// X6 source badge (derived desktop-side by plugin_source_for_path).
const SOURCE_BADGE: Record<PluginSource, { icon: string; class: string }> = {
  git: { icon: "sync", class: "bg-secondary/15 text-secondary" },
  local: { icon: "folder_open", class: "bg-surface-container-high text-on-surface-variant" },
  migration: { icon: "move_to_inbox", class: "bg-tertiary/20 text-tertiary" },
};

function sourceLabel(src: CatalogSource): string {
  switch (src.type) {
    case "mcp_registry":
      return `MCP Registry · ${src.publisher}`;
    case "featured_vendor":
      return "Shannon Featured";
    case "git_hub_repo":
      return `github.com/${src.repo}`;
    case "custom":
      return src.url;
    case "native":
      return "Native";
  }
}

export default function Plugins() {
  const intl = useIntl();
  const t = (id: string, values?: Record<string, string | number>) => intl.formatMessage({ id }, values);
  const { search } = useOutletContext<{ search: string }>();

  const [entries, setEntries] = useState<CatalogEntry[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [trustFilter, setTrustFilter] = useState<TrustFilter>("all");
  const [sourceFilter, setSourceFilter] = useState<SourceFilter>("all");
  const [sortMode, setSortMode] = useState<SortMode>("trust");
  const [installTarget, setInstallTarget] = useState<CatalogEntry | null>(null);
  const [upstreams, setUpstreams] = useState<CatalogUpstream[]>([]);

  // --- X6 installed management state ---
  const [installed, setInstalled] = useState<PluginInfo[]>([]);
  const [installedLoading, setInstalledLoading] = useState(true);
  const [installedError, setInstalledError] = useState(false);
  const [addMenuOpen, setAddMenuOpen] = useState(false);
  const [gitDialogOpen, setGitDialogOpen] = useState(false);
  // Name of the installed row whose lifecycle op is in flight (switch/update).
  const [busyName, setBusyName] = useState<string | null>(null);
  // Uninstall confirm: the row plus its inspected bundle preview.
  const [confirmTarget, setConfirmTarget] = useState<PluginInfo | null>(null);
  const [confirmSummary, setConfirmSummary] = useState<PluginBundleSummary | null>(null);
  const [confirmInspectFailed, setConfirmInspectFailed] = useState(false);
  const [uninstalling, setUninstalling] = useState(false);

  // NOTE: deps intentionally empty — this fetches once on mount. `t` is
  // recreated every render (intl.formatMessage closure), so including it
  // causes an infinite re-fetch loop: setEntries → re-render → new `t` →
  // effect re-fires → setEntries → ... (the original flicker bug).
  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    setError(null);
    api
      .listPluginMarketplace()
      .then((rows) => {
        if (cancelled) return;
        setEntries(rows);
      })
      .catch((e) => {
        if (cancelled) return;
        console.warn("listPluginMarketplace error:", e);
        setError(intl.formatMessage({ id: "extensions.plugins.loadError" }));
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const refreshInstalled = useCallback(() => {
    api
      .listPlugins()
      .then((rows) => {
        setInstalled(rows);
        setInstalledError(false);
      })
      .catch((e) => {
        console.warn("listPlugins error:", e);
        setInstalledError(true);
      })
      .finally(() => setInstalledLoading(false));
  }, []);

  useEffect(() => {
    refreshInstalled();
  }, [refreshInstalled]);

  // Stay in sync with installs started elsewhere (e.g. the market grid's
  // InstallDialog dispatches `shannon:extension-installed` on success).
  useEffect(() => {
    const handler = () => refreshInstalled();
    window.addEventListener("shannon:extension-installed", handler);
    return () => window.removeEventListener("shannon:extension-installed", handler);
  }, [refreshInstalled]);

  // Announce an install from this page's own add-flows so the other
  // extension tabs (Installed, Featured, …) refresh too — the same contract
  // InstallDialog honors.
  const dispatchInstalled = (name: string) => {
    window.dispatchEvent(
      new CustomEvent("shannon:extension-installed", {
        detail: { kind: "plugin", name },
      }),
    );
    refreshInstalled();
  };

  const runInstall = async (install: () => Promise<api.PluginInstallResult>) => {
    try {
      const result = await install();
      toast.success(t("extensions.plugins.installSuccess", { name: result.name }));
      if (result.warnings.length > 0) {
        toast.warning(result.warnings.join("\n"));
      }
      dispatchInstalled(result.name);
    } catch (e) {
      console.error("Plugin install error:", e);
      toast.error(
        t("extensions.plugins.installError", { error: safeErrorMessage(e, "install failed") }),
      );
    }
  };

  const handleAddLocal = async () => {
    try {
      const selected = await openDialog({
        directory: true,
        multiple: false,
        title: t("extensions.plugins.addLocal.dialogTitle"),
      });
      if (typeof selected !== "string" || !selected) return;
      await runInstall(() => api.installPlugin(selected));
    } catch (e) {
      console.warn("plugin directory picker error:", e);
    }
  };

  const handleAddArchive = async () => {
    try {
      const selected = await openDialog({
        multiple: false,
        title: t("extensions.plugins.addArchive.dialogTitle"),
        filters: [
          {
            name: t("extensions.plugins.addArchive.filterName"),
            extensions: ["dxt", "mcpb", "zip"],
          },
        ],
      });
      if (typeof selected !== "string" || !selected) return;
      await runInstall(() => api.installPlugin(selected));
    } catch (e) {
      console.warn("plugin archive picker error:", e);
    }
  };

  const handleToggle = async (plugin: PluginInfo, next: boolean) => {
    setBusyName(plugin.name);
    try {
      const result = next
        ? await api.enablePlugin(plugin.name)
        : await api.disablePlugin(plugin.name);
      toast.success(
        t(next ? "extensions.plugins.enabledToast" : "extensions.plugins.disabledToast", {
          name: plugin.name,
        }),
      );
      if (result.warnings.length > 0) {
        toast.warning(result.warnings.join("\n"));
      }
      refreshInstalled();
    } catch (e) {
      console.error("plugin enable/disable error:", e);
      toast.error(t("extensions.plugins.actionError", { error: safeErrorMessage(e, "action failed") }));
    } finally {
      setBusyName(null);
    }
  };

  const handleUpdate = async (plugin: PluginInfo) => {
    setBusyName(plugin.name);
    try {
      const result = await api.updatePlugin(plugin.name);
      toast.success(t("extensions.plugins.updatedToast", { name: plugin.name }));
      if (result.warnings.length > 0) {
        toast.warning(result.warnings.join("\n"));
      }
      refreshInstalled();
    } catch (e) {
      console.error("plugin update error:", e);
      toast.error(t("extensions.plugins.actionError", { error: safeErrorMessage(e, "action failed") }));
    } finally {
      setBusyName(null);
    }
  };

  const requestUninstall = (plugin: PluginInfo) => {
    setConfirmSummary(null);
    setConfirmInspectFailed(false);
    setConfirmTarget(plugin);
  };

  // Preview what the uninstall will remove by inspecting the plugin's own
  // directory. Failure is not fatal — the confirm dialog then says honestly
  // that only the registry entry will be removed.
  useEffect(() => {
    if (!confirmTarget) return;
    let cancelled = false;
    api
      .inspectPluginSource(confirmTarget.path)
      .then((summary) => {
        if (!cancelled) setConfirmSummary(summary);
      })
      .catch((e) => {
        console.warn("inspect for uninstall failed:", e);
        if (!cancelled) setConfirmInspectFailed(true);
      });
    return () => {
      cancelled = true;
    };
  }, [confirmTarget]);

  const handleUninstall = async () => {
    if (!confirmTarget) return;
    const name = confirmTarget.name;
    setUninstalling(true);
    try {
      const result = await api.uninstallPlugin(name);
      toast.success(t("extensions.plugins.uninstalledToast", { name }));
      if (result.warnings.length > 0) {
        toast.warning(result.warnings.join("\n"));
      }
      setConfirmTarget(null);
      refreshInstalled();
    } catch (e) {
      console.error("plugin uninstall error:", e);
      toast.error(t("extensions.plugins.actionError", { error: safeErrorMessage(e, "uninstall failed") }));
    } finally {
      setUninstalling(false);
    }
  };

  const addMenuItems: DropdownMenuItem[] = [
    {
      id: "add-plugin-git",
      label: t("extensions.plugins.add.git"),
      icon: "cloud_download",
      onSelect: () => setGitDialogOpen(true),
    },
    {
      id: "add-plugin-local",
      label: t("extensions.plugins.add.local"),
      icon: "folder_open",
      onSelect: handleAddLocal,
    },
    {
      id: "add-plugin-archive",
      label: t("extensions.plugins.add.archive"),
      icon: "archive",
      onSelect: handleAddArchive,
    },
  ];

  useEffect(() => {
    let cancelled = false;
    api
      .listCatalogUpstreams()
      .then((rows) => {
        if (cancelled) return;
        // Correlate entry_count by matching repo → entries' GitHubRepo source.
        const repoCounts = new Map<string, number>();
        for (const e of entries) {
          if (e.source?.type === "git_hub_repo" && e.source.repo) {
            repoCounts.set(e.source.repo, (repoCounts.get(e.source.repo) ?? 0) + 1);
          }
        }
        setUpstreams(
          rows.map((u) =>
            u.repo
              ? { ...u, entry_count: repoCounts.get(u.repo) ?? u.entry_count }
              : u,
          ),
        );
      })
      .catch((e) => console.warn("listCatalogUpstreams error:", e));
    return () => {
      cancelled = true;
    };
  }, [entries]);

  const filtered = useMemo(() => {
    const q = search.trim().toLowerCase();
    return entries.filter((e) => {
      if (trustFilter !== "all" && e.trust !== trustFilter) return false;
      if (sourceFilter !== "all" && e.source?.type !== sourceFilter) return false;
      if (!q) return true;
      const hay = [e.name, e.description, e.author ?? "", (e.tags ?? []).join(" "), sourceLabel(e.source)].join(" ").toLowerCase();
      return hay.includes(q);
    });
  }, [entries, trustFilter, sourceFilter, search]);

  const activeFilterCount =
    (trustFilter !== "all" ? 1 : 0) +
    (sourceFilter !== "all" ? 1 : 0) +
    (search.trim() ? 1 : 0);

  const resetFilters = () => {
    setTrustFilter("all");
    setSourceFilter("all");
  };

  const sorted = useMemo(() => {
    const sortFn = (a: CatalogEntry, b: CatalogEntry): number => {
      switch (sortMode) {
        case "trust": {
          const trustDiff = TRUST_ORDER[a.trust] - TRUST_ORDER[b.trust];
          if (trustDiff !== 0) return trustDiff;
          return a.name.localeCompare(b.name);
        }
        case "stars": {
          const aStars = a.stars ?? -1;
          const bStars = b.stars ?? -1;
          if (aStars !== bStars) return bStars - aStars;
          return a.name.localeCompare(b.name);
        }
        case "name":
          return a.name.localeCompare(b.name);
        case "recent": {
          const aDate = a.last_updated ? new Date(a.last_updated).getTime() : 0;
          const bDate = b.last_updated ? new Date(b.last_updated).getTime() : 0;
          if (aDate !== bDate) return bDate - aDate;
          return a.name.localeCompare(b.name);
        }
        default:
          return 0;
      }
    };
    return [...filtered].sort(sortFn);
  }, [filtered, sortMode]);

  const handleInstall = (entry: CatalogEntry) => {
    // All install flows route through the InstallDialog so the user can see
    // the source-provided config before committing.
    setInstallTarget(entry);
  };

  // Bundle checklist rows for the uninstall confirm, reusing the X5 bundle
  // preview strings (they are removal-neutral: "{count} skills: {names}").
  const confirmRows = confirmSummary
    ? [
        {
          testid: "uninstall-skills",
          id: "extensions.installDialog.bundle.skills",
          count: confirmSummary.skills.length,
          names: confirmSummary.skills.join(", "),
        },
        {
          testid: "uninstall-agents",
          id: "extensions.installDialog.bundle.agents",
          count: confirmSummary.agents.length,
          names: confirmSummary.agents.join(", "),
        },
        {
          testid: "uninstall-commands",
          id: "extensions.installDialog.bundle.commands",
          count: confirmSummary.commands.length,
          names: confirmSummary.commands.join(", "),
        },
        {
          testid: "uninstall-mcp",
          id: "extensions.installDialog.bundle.mcp",
          count: confirmSummary.mcp_servers.length,
          names: confirmSummary.mcp_servers.join(", "),
        },
      ].filter((r) => r.count > 0)
    : [];

  const renderInstalledRow = (plugin: PluginInfo) => {
    const badge = SOURCE_BADGE[plugin.source];
    const migration = plugin.migration_imported;
    const busy = busyName === plugin.name;
    const migrationTooltip = t("extensions.plugins.installed.migrationTooltip");
    return (
      <li
        key={plugin.name}
        data-testid={`installed-row-${plugin.name}`}
        className="border border-outline-variant/40 rounded-xl px-md py-sm bg-surface-container-lowest flex items-center gap-md"
      >
        <div className="w-8 h-8 rounded-lg bg-primary/10 text-primary flex items-center justify-center shrink-0">
          <span className="material-symbols-outlined icon-sm">workspaces</span>
        </div>
        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-xs flex-wrap">
            <span className="font-bold text-label-md text-on-surface truncate">{plugin.name}</span>
            {plugin.version && (
              <span className="text-label-xs text-on-surface-variant">{plugin.version}</span>
            )}
            <span
              className={cn(
                "inline-flex items-center gap-[2px] px-xs py-[1px] rounded-full text-label-xs font-bold",
                badge.class,
              )}
            >
              <span className="material-symbols-outlined icon-xs">{badge.icon}</span>
              {t(`extensions.plugins.installed.source.${plugin.source}`)}
            </span>
            <span
              className="inline-flex items-center px-xs py-[1px] rounded bg-surface-container-high text-label-xs font-mono text-on-surface-variant"
              title={t("extensions.plugins.installed.sourceFormat", { format: plugin.source_format })}
            >
              {plugin.source_format}
            </span>
          </div>
          {plugin.description && (
            <p className="text-label-sm text-on-surface-variant truncate">{plugin.description}</p>
          )}
        </div>

        {/* Migration records are informational: their imported originals
            live in the per-type tabs, so uninstall/enable/disable must not
            be offered here. */}
        <span title={migration ? migrationTooltip : undefined} className="inline-flex">
          <Switch
            size="sm"
            checked={plugin.enabled}
            disabled={migration || busy}
            onCheckedChange={(next) => handleToggle(plugin, next === true)}
            aria-label={t("extensions.plugins.installed.toggleAria", { name: plugin.name })}
            data-testid={`installed-toggle-${plugin.name}`}
          />
        </span>

        {/* 更新 is a git pull under the hood — only meaningful for git-sourced
            plugins (exactly the rows the backend's `.git` check accepts). */}
        {plugin.source === "git" && (
          <Button
            variant="secondary"
            size="sm"
            disabled={busy}
            onClick={() => handleUpdate(plugin)}
            aria-label={t("extensions.plugins.installed.updateAria", { name: plugin.name })}
            data-testid={`installed-update-${plugin.name}`}
            className="px-sm py-xs rounded-lg cursor-pointer"
          >
            <span className="material-symbols-outlined text-[14px]">sync</span>
            {t("extensions.plugins.installed.update")}
          </Button>
        )}

        <span title={migration ? migrationTooltip : undefined} className="inline-flex">
          <Button
            variant="ghost"
            size="sm"
            disabled={migration || busy}
            onClick={() => requestUninstall(plugin)}
            aria-label={t("extensions.plugins.installed.uninstallAria", { name: plugin.name })}
            data-testid={`installed-uninstall-${plugin.name}`}
            className="px-sm py-xs rounded-lg text-on-surface-variant hover:text-error hover:bg-error/10 cursor-pointer"
          >
            <span className="material-symbols-outlined text-[16px]">delete</span>
          </Button>
        </span>
      </li>
    );
  };

  const renderCard = (entry: CatalogEntry) => {
    const stars = entry.stars ?? null;
    const license = entry.license ?? null;
    const trust = entry.trust;
    return (
      <div
        key={entry.id}
        className="border border-outline-variant/40 rounded-2xl p-md bg-surface-container-lowest hover:border-primary/50 hover:shadow-md transition-all flex flex-col gap-sm"
      >
        <div className="flex items-start justify-between gap-sm">
          <div className="flex items-start gap-sm min-w-0">
            <div className="w-9 h-9 rounded-lg bg-primary/10 text-primary flex items-center justify-center shrink-0">
              <span className="material-symbols-outlined icon-md">workspaces</span>
            </div>
            <div className="min-w-0">
              <h4 className="font-bold text-label-md text-on-surface truncate">{entry.name}</h4>
              <p className="text-label-xs text-on-surface-variant truncate">{entry.author ?? sourceLabel(entry.source)}</p>
            </div>
          </div>
          <span
            className={cn("inline-flex items-center gap-xs px-xs py-[2px] rounded-full text-label-xs font-bold shrink-0", TRUST_BADGE_CLASS[trust])}
            title={t(TRUST_LABEL_KEY[trust])}
          >
            <span className="material-symbols-outlined icon-xs">{TRUST_ICON[trust]}</span>
            {t(TRUST_LABEL_KEY[trust])}
          </span>
        </div>

        <p className="text-label-sm text-on-surface-variant line-clamp-3 min-h-[40px]">
          {entry.description || t("extensions.plugins.noDescription")}
        </p>

        <div className="flex flex-wrap items-center gap-xs text-label-xs text-on-surface-variant">
          {license && (
            <span className="inline-flex items-center gap-[2px] px-xs py-[1px] rounded bg-surface-container-high">
              <span className="material-symbols-outlined icon-xs">gavel</span>
              {license}
            </span>
          )}
          {typeof stars === "number" && (
            <span className="inline-flex items-center gap-[2px] px-xs py-[1px] rounded bg-surface-container-high">
              <span className="material-symbols-outlined icon-xs">star</span>
              {stars >= 1000 ? `${(stars / 1000).toFixed(1)}k` : stars}
            </span>
          )}
          {entry.version && (
            <span className="inline-flex items-center gap-[2px] px-xs py-[1px] rounded bg-surface-container-high">
              <span className="material-symbols-outlined icon-xs">tag</span>
              {entry.version}
            </span>
          )}
          <span className="inline-flex items-center gap-[2px] truncate" title={sourceLabel(entry.source)}>
            <span className="material-symbols-outlined icon-xs">link</span>
            <span className="truncate">{sourceLabel(entry.source)}</span>
          </span>
        </div>

        <div className="flex items-center justify-between gap-sm pt-xs">
          {entry.homepage_url ? (
            <a
              href={entry.homepage_url}
              target="_blank"
              rel="noopener noreferrer"
              className="text-label-sm text-link hover:underline inline-flex items-center gap-xs"
            >
              <span className="material-symbols-outlined text-[14px]">open_in_new</span>
              {t("extensions.plugins.homepage")}
            </a>
          ) : (
            <span />
          )}
          <Button
            onClick={() => handleInstall(entry)}
            className="px-md py-xs rounded-lg hover:bg-primary/90 cursor-pointer focus-visible:ring-2 focus-visible:ring-primary/30"
          >
            <span className="material-symbols-outlined text-[14px]">download</span>
            {t("extensions.plugins.install")}
          </Button>
        </div>
      </div>
    );
  };

  return (
    <div className="p-lg max-w-7xl mx-auto">
      <div className="text-center py-xl">
        <div className="inline-flex items-center justify-center w-16 h-16 rounded-full bg-primary/10 mb-md">
          <span className="material-symbols-outlined text-primary text-[32px]">workspaces</span>
        </div>
        <h2 className="text-headline-md font-headline-md text-on-surface mb-sm">
          {intl.formatMessage({ id: "extensions.plugins.title" })}
        </h2>
        <p className="text-body-md text-on-surface-variant max-w-xl mx-auto">
          <FormattedMessage id="extensions.plugins.descriptionLive" />
        </p>
      </div>

      {/* --- X6 installed management --- */}
      <section
        data-testid="plugins-installed-section"
        aria-label={t("extensions.plugins.installed.section")}
        className="mb-xl"
      >
        <div className="flex items-center justify-between gap-md mb-md flex-wrap">
          <div className="flex items-baseline gap-sm min-w-0">
            <h3 className="text-label-sm font-bold text-on-surface-variant uppercase tracking-widest">
              {t("extensions.plugins.installed.section")}
            </h3>
            {!installedLoading && !installedError && installed.length > 0 && (
              <span className="text-label-xs text-on-surface-variant">
                {t("extensions.plugins.installed.count", { count: installed.length })}
              </span>
            )}
          </div>
          <span className="relative inline-flex">
            <Button
              onClick={() => setAddMenuOpen((open) => !open)}
              aria-haspopup="menu"
              aria-expanded={addMenuOpen}
              aria-label={t("extensions.plugins.add.aria")}
              data-testid="add-plugin-button"
              className="px-md py-xs rounded-lg hover:bg-primary/90 cursor-pointer focus-visible:ring-2 focus-visible:ring-primary/30"
            >
              <span className="material-symbols-outlined text-[14px]">add</span>
              {t("extensions.plugins.add.label")}
            </Button>
            <DropdownMenu
              open={addMenuOpen}
              onClose={() => setAddMenuOpen(false)}
              items={addMenuItems}
              ariaLabel={t("extensions.plugins.add.aria")}
            />
          </span>
        </div>

        {installedLoading ? (
          <LoadingState size="sm" label={t("extensions.plugins.installed.loading")} />
        ) : installedError ? (
          // A2 polish: the installed section had the CATALOG error title
          // (「Could not load catalog」) — wrong surface, and no way out. Its
          // own title key plus a 重试 that re-runs the installed list fetch.
          <ErrorState
            icon="cloud_off"
            title={t("extensions.plugins.installed.loadFailed")}
            description={t("extensions.plugins.installed.loadError")}
            action={{
              label: t("extensions.plugins.installed.retry"),
              onClick: refreshInstalled,
            }}
          />
        ) : installed.length === 0 ? (
          <EmptyState
            compact
            icon="package_2"
            title={t("extensions.plugins.installed.empty")}
            description={t("extensions.plugins.installed.emptyHint")}
          />
        ) : (
          <ul className="flex flex-col gap-sm">
            {installed.map(renderInstalledRow)}
          </ul>
        )}
      </section>

      {upstreams.length > 0 && (
        <div className="mb-lg">
          <h3 className="text-label-sm font-bold text-on-surface-variant uppercase tracking-widest mb-sm">
            {t("extensions.plugins.source.label")}
          </h3>
          <div className="flex flex-wrap gap-xs">
            {upstreams.map((u) => (
              <a
                key={`${u.kind}:${u.slug}`}
                href={u.repo ? `https://github.com/${u.repo}` : undefined}
                target="_blank"
                rel="noopener noreferrer"
                className="inline-flex items-center gap-xs px-sm py-xs rounded-full bg-surface-container-low text-on-surface-variant text-label-sm hover:bg-surface-container-high transition-colors"
                title={u.repo ? `github.com/${u.repo}` : u.display_name}
              >
                <span className="material-symbols-outlined text-[14px] text-primary">
                  {u.kind === "skill"
                    ? "extension"
                    : u.kind === "agent"
                      ? "smart_toy"
                      : u.kind === "mcp"
                        ? "cloud"
                        : u.kind === "native"
                          ? "stars"
                          : "database"}
                </span>
                <span className="font-bold text-on-surface">{u.display_name}</span>
                <span className="text-label-xs text-on-surface-variant">
                  · {u.trust}
                  {u.entry_count > 0 && ` · ${u.entry_count}`}
                </span>
              </a>
            ))}
          </div>
        </div>
      )}

      <div className="flex items-center justify-between gap-md mb-lg flex-wrap">
        <div className="flex items-center gap-xs flex-wrap">
          <select
            value={trustFilter}
            onChange={(e) => setTrustFilter(e.target.value as TrustFilter)}
            aria-label={t("extensions.plugins.filter.trust.label")}
            className="px-sm py-xs rounded-lg bg-surface-container-low text-on-surface text-label-sm font-bold cursor-pointer hover:bg-surface-container-high transition-colors"
          >
            <option value="all">{t("extensions.plugins.filter.trust.all")}</option>
            {TRUST_FILTER_ORDER.filter((x) => x !== "all").map((tf) => (
              <option key={tf} value={tf}>{t(TRUST_LABEL_KEY[tf])}</option>
            ))}
          </select>
          <select
            value={sourceFilter}
            onChange={(e) => setSourceFilter(e.target.value as SourceFilter)}
            aria-label={t("extensions.plugins.filter.source.label")}
            className="px-sm py-xs rounded-lg bg-surface-container-low text-on-surface text-label-sm font-bold cursor-pointer hover:bg-surface-container-high transition-colors"
          >
            <option value="all">{t("extensions.plugins.filter.source.all")}</option>
            {SOURCE_FILTERS.filter((x) => x !== "all").map((sf) => (
              <option key={sf} value={sf}>{t(`extensions.plugins.filter.source.${sf === "git_hub_repo" ? "github" : sf === "featured_vendor" ? "featured" : sf}`)}</option>
            ))}
          </select>
          <span className="text-label-sm text-on-surface-variant">{t("extensions.plugins.sortLabel")}</span>
          <select
            value={sortMode}
            onChange={(e) => setSortMode(e.target.value as SortMode)}
            aria-label={t("extensions.plugins.sortLabel")}
            className="px-sm py-xs rounded-lg bg-surface-container-low text-on-surface text-label-sm font-bold cursor-pointer hover:bg-surface-container-high transition-colors"
          >
            <option value="trust">{t("extensions.plugins.sortTrust")}</option>
            <option value="stars">{t("extensions.plugins.sortStars")}</option>
            <option value="name">{t("extensions.plugins.sortName")}</option>
            <option value="recent">{t("extensions.plugins.sortRecent")}</option>
          </select>
        </div>
      </div>

      {(activeFilterCount > 0 || filtered.length === 0) && (
        <div className="flex items-center justify-between gap-md mb-md flex-wrap">
          <span className="text-label-sm text-on-surface-variant">
            <FormattedMessage id="extensions.plugins.count" values={{ count: filtered.length }} />
            {activeFilterCount > 0 && (
              <span className="ml-sm text-on-surface-variant/70">
                · <FormattedMessage id="extensions.plugins.filter.active" values={{ count: activeFilterCount }} />
              </span>
            )}
          </span>
          {activeFilterCount > 0 && (
            <Button
              variant="secondary"
              size="sm"
              onClick={resetFilters}
              className="px-sm py-xs rounded-lg hover:bg-surface-container-high focus-visible:ring-2 focus-visible:ring-primary/30"
            >
              <span className="material-symbols-outlined text-[14px]">filter_alt_off</span>
              {t("extensions.plugins.filter.reset")}
            </Button>
          )}
        </div>
      )}

      {loading ? (
        <div className="grid grid-cols-1 md:grid-cols-2 gap-md">
          {Array.from({ length: 4 }).map((_, i) => <CardSkeleton key={i} />)}
        </div>
      ) : error ? (
        <ErrorState
          icon="cloud_off"
          title={t("extensions.plugins.loadFailed")}
          description={error}
        />
      ) : filtered.length === 0 ? (
        <EmptyState
          icon="search_off"
          title={search ? t("extensions.plugins.noMatch") : t("extensions.plugins.empty")}
        />
      ) : (
        <div className="grid grid-cols-1 md:grid-cols-2 xl:grid-cols-3 gap-md">
          {sorted.map(renderCard)}
        </div>
      )}

      <InstallDialog
        entry={installTarget}
        open={!!installTarget}
        onClose={() => setInstallTarget(null)}
        onInstalled={() => {
          // The dialog already emits `shannon:extension-installed`; the
          // listener above refreshes the installed section on it.
          refreshInstalled();
        }}
      />

      <AddPluginGitDialog
        open={gitDialogOpen}
        onClose={() => setGitDialogOpen(false)}
        onInstalled={(result) => dispatchInstalled(result.name)}
      />

      {/* X6 uninstall confirm — consent via listing. The dialog previews the
          bundle by inspecting the plugin's directory; when the inspection
          fails it says honestly that only the registry entry will be
          removed instead of inventing a list. */}
      <Modal
        open={!!confirmTarget}
        onClose={() => {
          if (!uninstalling) setConfirmTarget(null);
        }}
        size="sm"
        role="alertdialog"
        title={t("extensions.plugins.uninstallConfirm.title", { name: confirmTarget?.name ?? "" })}
        busy={uninstalling}
        showCloseButton={false}
        testId="uninstall-plugin-dialog"
      >
        <ModalBody className="flex flex-col gap-sm">
          <div className="flex items-start gap-sm">
            <span className="material-symbols-outlined icon-lg text-error mt-[2px]" aria-hidden="true">
              warning
            </span>
            <p className="text-body-md text-on-surface-variant">
              {t("extensions.plugins.uninstallConfirm.message")}
            </p>
          </div>
          {confirmInspectFailed ? (
            <p
              data-testid="uninstall-inspect-failed"
              className="text-label-sm text-on-warning-container bg-warning-container/40 rounded-md px-sm py-xs"
            >
              {t("extensions.plugins.uninstallConfirm.inspectFailed")}
            </p>
          ) : confirmSummary ? (
            <div
              data-testid="uninstall-list"
              className="rounded-xl border border-outline-variant/30 bg-surface-container-low/60 p-md flex flex-col gap-xs"
            >
              {confirmRows.length === 0 ? (
                <p data-testid="uninstall-list-empty" className="text-label-sm text-on-surface-variant">
                  {t("extensions.plugins.uninstallConfirm.empty")}
                </p>
              ) : (
                confirmRows.map((row) => (
                  <div key={row.id} data-testid={row.testid} className="flex items-start gap-xs text-label-sm text-on-surface-variant">
                    <span className="material-symbols-outlined text-[14px] mt-[2px]" aria-hidden="true">
                      remove_circle
                    </span>
                    <span>{intl.formatMessage({ id: row.id }, { count: row.count, names: row.names })}</span>
                  </div>
                ))
              )}
              <p className="text-label-xs text-on-surface-variant">
                {t("extensions.plugins.uninstallConfirm.dirNote")}
              </p>
            </div>
          ) : (
            <p data-testid="uninstall-inspecting" className="text-label-sm text-on-surface-variant">
              <FormattedMessage id="extensions.installDialog.bundle.inspecting" />
            </p>
          )}
        </ModalBody>
        <ModalFooter className="pt-0">
          <Button
            variant="ghost"
            disabled={uninstalling}
            onClick={() => setConfirmTarget(null)}
            className="px-md py-sm rounded-xl text-on-surface-variant hover:bg-surface-container cursor-pointer"
          >
            {t("extensions.plugins.uninstallConfirm.cancel")}
          </Button>
          <Button
            data-testid="uninstall-confirm-button"
            disabled={uninstalling || (!confirmInspectFailed && confirmSummary === null)}
            onClick={handleUninstall}
            className="px-md py-sm rounded-xl bg-error hover:bg-error/90 text-on-error cursor-pointer disabled:opacity-50"
          >
            {t("extensions.plugins.uninstallConfirm.confirm")}
          </Button>
        </ModalFooter>
      </Modal>
    </div>
  );
}
