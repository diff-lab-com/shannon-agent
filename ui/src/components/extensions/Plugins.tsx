import { useEffect, useMemo, useState } from "react";
import { FormattedMessage, useIntl } from "react-intl";
import { useOutletContext } from "react-router-dom";
import * as api from "@/lib/tauri-api";
import type { CatalogUpstream } from "@/lib/tauri-api";
import { CardSkeleton } from "@/components/SkeletonLoader";
import ErrorState from "@/components/ui/error-state";
import EmptyState from "@/components/ui/empty-state";
import type { CatalogEntry, CatalogSource, TrustLevel } from "@/types";
import InstallDialog from "./InstallDialog";

type SortMode = "trust" | "stars" | "name" | "recent";
type TrustFilter = TrustLevel | "all";
type SourceFilter = CatalogSource["type"] | "all";

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
  const [trustFilter, setTrustFilter] = useState<TrustFilter>("all");
  const [sourceFilter, setSourceFilter] = useState<SourceFilter>("all");
  const [sortMode, setSortMode] = useState<SortMode>("trust");
  const [installTarget, setInstallTarget] = useState<CatalogEntry | null>(null);
  const [upstreams, setUpstreams] = useState<CatalogUpstream[]>([]);

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
            className={`inline-flex items-center gap-xs px-xs py-[2px] rounded-full text-label-xs font-bold shrink-0 ${TRUST_BADGE_CLASS[trust]}`}
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
            className="px-md py-xs rounded-lg bg-primary text-on-primary text-label-sm font-bold hover:bg-primary/90 inline-flex items-center gap-xs cursor-pointer focus:outline-none focus-visible:ring-2 focus-visible:ring-primary/30"
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
        <h2 className="text-headline-md font-headline-md text-on-surface mb-sm">
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
            <button
              onClick={resetFilters}
              className="inline-flex items-center gap-xs px-sm py-xs rounded-lg bg-surface-container-low text-on-surface-variant hover:bg-surface-container-high transition-colors text-label-sm font-bold focus:outline-none focus-visible:ring-2 focus-visible:ring-primary/30"
            >
              <span className="material-symbols-outlined text-[14px]">filter_alt_off</span>
              {t("extensions.plugins.filter.reset")}
            </button>
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
          // The dialog already emits `shannon:extension-installed`; this
          // callback is a hook for future parent-side refresh logic.
        }}
      />
    </div>
  );
}
