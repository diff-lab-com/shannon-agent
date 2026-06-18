import { useEffect, useMemo, useState } from "react";
import { FormattedMessage, useIntl } from "react-intl";
import { useOutletContext } from "react-router-dom";
import * as api from "@/lib/tauri-api";
import type { CatalogUpstream } from "@/lib/tauri-api";
import type { AddonKind, CatalogEntry, CatalogSource, TrustLevel } from "@/types";
import InstallDialog from "./InstallDialog";

const KIND_ORDER: AddonKind[] = ["mcp", "skill", "agent", "data_source", "plugin"];

type SortMode = "trust" | "stars" | "name" | "recent";

const KIND_ICON: Record<AddonKind, string> = {
  mcp: "cloud",
  skill: "extension",
  agent: "smart_toy",
  data_source: "database",
  plugin: "workspaces",
};

const KIND_LABEL_KEY: Record<AddonKind, string> = {
  mcp: "extensions.plugins.kindMcp",
  skill: "extensions.plugins.kindSkill",
  agent: "extensions.plugins.kindAgent",
  data_source: "extensions.plugins.kindDataSource",
  plugin: "extensions.plugins.kindPlugin",
};

export const KIND_ROUTE: Partial<Record<AddonKind, string>> = {
  mcp: "/extensions/mcp-servers",
  skill: "/extensions/skills",
  agent: "/extensions/agents",
  data_source: "/extensions/datasources",
};

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
  const t = (id: string) => intl.formatMessage({ id });
  const { search } = useOutletContext<{ search: string }>();

  const [entries, setEntries] = useState<CatalogEntry[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [kindFilter, setKindFilter] = useState<AddonKind | "all">("all");
  const [sortMode, setSortMode] = useState<SortMode>("trust");
  const [installTarget, setInstallTarget] = useState<CatalogEntry | null>(null);
  const [upstreams, setUpstreams] = useState<CatalogUpstream[]>([]);

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
        setError(t("extensions.plugins.loadError"));
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [t]);

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
      if (kindFilter !== "all" && e.kind !== kindFilter) return false;
      if (!q) return true;
      const hay = [e.name, e.description, e.author ?? "", (e.tags ?? []).join(" "), sourceLabel(e.source)].join(" ").toLowerCase();
      return hay.includes(q);
    });
  }, [entries, kindFilter, search]);

  const grouped = useMemo(() => {
    const map = new Map<AddonKind, CatalogEntry[]>();
    for (const e of filtered) {
      const list = map.get(e.kind) ?? [];
      list.push(e);
      map.set(e.kind, list);
    }

    // Sort within each kind group based on sortMode
    const sortFn = (a: CatalogEntry, b: CatalogEntry) => {
      switch (sortMode) {
        case "trust":
          // Lower trust order number = higher trust (verified=0, official=1, etc.)
          const trustDiff = TRUST_ORDER[a.trust] - TRUST_ORDER[b.trust];
          if (trustDiff !== 0) return trustDiff;
          // Tie-break by name
          return a.name.localeCompare(b.name);
        case "stars":
          // More stars first, nulls last
          const aStars = a.stars ?? -1;
          const bStars = b.stars ?? -1;
          if (aStars !== bStars) return bStars - aStars;
          return a.name.localeCompare(b.name);
        case "name":
          return a.name.localeCompare(b.name);
        case "recent":
          // More recent first, nulls last
          const aDate = a.last_updated ? new Date(a.last_updated).getTime() : 0;
          const bDate = b.last_updated ? new Date(b.last_updated).getTime() : 0;
          if (aDate !== bDate) return bDate - aDate;
          return a.name.localeCompare(b.name);
        default:
          return 0;
      }
    };

    return KIND_ORDER.filter((k) => map.has(k)).map((k) => ({
      kind: k,
      rows: map.get(k)!.sort(sortFn)
    }));
  }, [filtered, sortMode]);

  const handleInstall = (entry: CatalogEntry) => {
    // All install flows route through the InstallDialog so the user can see
    // the source-provided config before committing.
    setInstallTarget(entry);
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
              <span className="material-symbols-outlined text-[20px]">{KIND_ICON[entry.kind]}</span>
            </div>
            <div className="min-w-0">
              <h4 className="font-bold text-label-md text-on-surface truncate">{entry.name}</h4>
              <p className="text-label-xs text-on-surface-variant truncate">{entry.author ?? sourceLabel(entry.source)}</p>
            </div>
          </div>
          <span
            className={`inline-flex items-center gap-xs px-xs py-[2px] rounded-full text-label-xs font-bold shrink-0 ${TRUST_BADGE_CLASS[trust]}`}
            title={t(TRUST_LABEL_KEY[trust])}
          >
            <span className="material-symbols-outlined text-[12px]">{TRUST_ICON[trust]}</span>
            {t(TRUST_LABEL_KEY[trust])}
          </span>
        </div>

        <p className="text-label-sm text-on-surface-variant line-clamp-3 min-h-[40px]">
          {entry.description || t("extensions.plugins.noDescription")}
        </p>

        <div className="flex flex-wrap items-center gap-xs text-label-xs text-on-surface-variant">
          {license && (
            <span className="inline-flex items-center gap-[2px] px-xs py-[1px] rounded bg-surface-container-high">
              <span className="material-symbols-outlined text-[12px]">gavel</span>
              {license}
            </span>
          )}
          {typeof stars === "number" && (
            <span className="inline-flex items-center gap-[2px] px-xs py-[1px] rounded bg-surface-container-high">
              <span className="material-symbols-outlined text-[12px]">star</span>
              {stars >= 1000 ? `${(stars / 1000).toFixed(1)}k` : stars}
            </span>
          )}
          {entry.version && (
            <span className="inline-flex items-center gap-[2px] px-xs py-[1px] rounded bg-surface-container-high">
              <span className="material-symbols-outlined text-[12px]">tag</span>
              {entry.version}
            </span>
          )}
          <span className="inline-flex items-center gap-[2px] truncate" title={sourceLabel(entry.source)}>
            <span className="material-symbols-outlined text-[12px]">link</span>
            <span className="truncate">{sourceLabel(entry.source)}</span>
          </span>
        </div>

        <div className="flex items-center justify-between gap-sm pt-xs">
          {entry.homepage_url ? (
            <a
              href={entry.homepage_url}
              target="_blank"
              rel="noopener noreferrer"
              className="text-label-sm text-primary hover:underline inline-flex items-center gap-xs"
            >
              <span className="material-symbols-outlined text-[14px]">open_in_new</span>
              {t("extensions.plugins.homepage")}
            </a>
          ) : (
            <span />
          )}
          <button
            onClick={() => handleInstall(entry)}
            className="px-md py-xs rounded-lg bg-primary text-on-primary text-label-sm font-bold hover:bg-primary/90 inline-flex items-center gap-xs cursor-pointer"
          >
            <span className="material-symbols-outlined text-[14px]">download</span>
            {t("extensions.plugins.install")}
          </button>
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
        <h2 className="text-headline-md font-bold text-on-surface mb-sm">
          {intl.formatMessage({ id: "extensions.plugins.title" })}
        </h2>
        <p className="text-body-md text-on-surface-variant max-w-xl mx-auto">
          <FormattedMessage id="extensions.plugins.descriptionLive" />
        </p>
      </div>

      {upstreams.length > 0 && (
        <div className="mb-lg">
          <h3 className="text-label-sm font-bold text-outline uppercase tracking-widest mb-sm">
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

      <div className="flex items-center justify-between gap-md mb-lg">
        <div className="flex items-center gap-xs flex-wrap">
          <button
            onClick={() => setKindFilter("all")}
            className={`px-sm py-xs rounded-full text-label-sm font-bold cursor-pointer transition-colors ${kindFilter === "all" ? "bg-primary text-on-primary" : "bg-surface-container-low text-on-surface-variant hover:bg-surface-container-high"}`}
          >
            {t("extensions.plugins.filterAll")}
          </button>
          {KIND_ORDER.map((k) => (
            <button
              key={k}
              onClick={() => setKindFilter(k)}
              className={`px-sm py-xs rounded-full text-label-sm font-bold cursor-pointer transition-colors inline-flex items-center gap-xs ${kindFilter === k ? "bg-primary text-on-primary" : "bg-surface-container-low text-on-surface-variant hover:bg-surface-container-high"}`}
            >
              <span className="material-symbols-outlined text-[14px]">{KIND_ICON[k]}</span>
              {t(KIND_LABEL_KEY[k])}
            </button>
          ))}
        </div>
        <div className="flex items-center gap-xs">
          <span className="text-label-sm text-on-surface-variant">{t("extensions.plugins.sortLabel")}</span>
          <select
            value={sortMode}
            onChange={(e) => setSortMode(e.target.value as SortMode)}
            className="px-sm py-xs rounded-lg bg-surface-container-low text-on-surface text-label-sm font-bold cursor-pointer hover:bg-surface-container-high transition-colors"
          >
            <option value="trust">{t("extensions.plugins.sortTrust")}</option>
            <option value="stars">{t("extensions.plugins.sortStars")}</option>
            <option value="name">{t("extensions.plugins.sortName")}</option>
            <option value="recent">{t("extensions.plugins.sortRecent")}</option>
          </select>
          <span className="text-label-sm text-on-surface-variant shrink-0">
            <FormattedMessage id="extensions.plugins.count" values={{ count: filtered.length }} />
          </span>
        </div>
      </div>

      {loading ? (
        <div className="flex items-center justify-center py-xl">
          <span className="material-symbols-outlined animate-spin text-[32px] text-primary">progress_activity</span>
        </div>
      ) : error ? (
        <div className="text-center py-xl text-on-surface-variant">
          <span className="material-symbols-outlined text-[32px] mb-sm block">cloud_off</span>
          <p className="text-label-md">{error}</p>
        </div>
      ) : filtered.length === 0 ? (
        <div className="text-center py-xl text-on-surface-variant">
          <span className="material-symbols-outlined text-[32px] mb-sm block">search_off</span>
          <p className="text-label-md">
            {search ? t("extensions.plugins.noMatch") : t("extensions.plugins.empty")}
          </p>
        </div>
      ) : (
        <div className="space-y-xl">
          {grouped.map(({ kind, rows }) => (
            <section key={kind}>
              <h3 className="text-label-sm font-bold text-outline uppercase tracking-widest mb-md inline-flex items-center gap-xs">
                <span className="material-symbols-outlined text-[14px]">{KIND_ICON[kind]}</span>
                {t(KIND_LABEL_KEY[kind])}
                <span className="text-on-surface-variant font-normal">· {rows.length}</span>
              </h3>
              <div className="grid grid-cols-1 md:grid-cols-2 xl:grid-cols-3 gap-md">
                {rows.map(renderCard)}
              </div>
            </section>
          ))}
        </div>
      )}

      <InstallDialog
        entry={installTarget}
        open={!!installTarget}
        onClose={() => setInstallTarget(null)}
        onInstalled={() => {
          // The dialog already emits `shannon:extension-installed`; this
          // callback is a hook for future parent-side refresh logic.
        }}
      />
    </div>
  );
}
