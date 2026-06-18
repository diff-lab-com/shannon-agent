import { useEffect, useMemo, useState } from "react";
import { FormattedMessage, useIntl } from "react-intl";
import { useOutletContext, useNavigate } from "react-router-dom";
import { toast } from "sonner";
import * as api from "@/lib/tauri-api";
import type { AddonKind, CatalogEntry, CatalogSource, TrustLevel } from "@/types";

const KIND_ORDER: AddonKind[] = ["mcp", "skill", "agent", "data_source", "plugin"];

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

const KIND_ROUTE: Partial<Record<AddonKind, string>> = {
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
  const navigate = useNavigate();
  const t = (id: string) => intl.formatMessage({ id });
  const { search } = useOutletContext<{ search: string }>();

  const [entries, setEntries] = useState<CatalogEntry[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [kindFilter, setKindFilter] = useState<AddonKind | "all">("all");
  const [installingId, setInstallingId] = useState<string | null>(null);

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
    return KIND_ORDER.filter((k) => map.has(k)).map((k) => ({ kind: k, rows: map.get(k)! }));
  }, [filtered]);

  const handleInstall = async (entry: CatalogEntry) => {
    const route = KIND_ROUTE[entry.kind];
    if (route) {
      navigate(route);
      toast.info(
        intl.formatMessage(
          { id: "extensions.plugins.installRouted" },
          { kind: t(KIND_LABEL_KEY[entry.kind]) },
        ),
      );
      return;
    }
    // Plugin bundles: no dedicated installer yet — prompt user to follow homepage.
    setInstallingId(entry.id);
    try {
      if (entry.homepage_url) {
        await api.saveTextFile(`${entry.name}.url.txt`, `InternetShortcut\nURL=${entry.homepage_url}\n`);
      }
      toast.success(t("extensions.plugins.installHint"));
    } catch (e) {
      console.warn("plugin install hint error:", e);
      toast.error(t("extensions.plugins.installFailed"));
    } finally {
      setInstallingId(null);
    }
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

        <p className="text-label-sm text-on-surface-variant line-clamp-2 min-h-[32px]">
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
            disabled={installingId === entry.id}
            className="px-md py-xs rounded-lg bg-primary text-on-primary text-label-sm font-bold hover:bg-primary/90 disabled:opacity-60 inline-flex items-center gap-xs cursor-pointer"
          >
            <span className="material-symbols-outlined text-[14px]">
              {installingId === entry.id ? "progress_activity" : "download"}
            </span>
            {installingId === entry.id ? t("extensions.plugins.installing") : t("extensions.plugins.install")}
          </button>
        </div>
      </div>
    );
  };

  return (
    <div className="p-lg max-w-6xl mx-auto">
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
        <span className="text-label-sm text-on-surface-variant shrink-0">
          <FormattedMessage id="extensions.plugins.count" values={{ count: filtered.length }} />
        </span>
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
              <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-md">
                {rows.map(renderCard)}
              </div>
            </section>
          ))}
        </div>
      )}
    </div>
  );
}
