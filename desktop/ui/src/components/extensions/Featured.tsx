import { useEffect, useState } from "react";
import { Spinner } from '@/components/ui/loading-state'
import { useOutletContext } from "react-router-dom";
import { useIntl } from 'react-intl'
import {
  listFeaturedVendors,
  listInstalledAddons,
  installMcpOAuthLoopback,
  installMcpOAuthComplete,
  installMcpStdio,
  type FeaturedVendor,
} from "@/lib/tauri-api";
import type { InstalledAddonSummary } from "@/types";
import { Button } from "@/components/ui/button";
import { CardSkeleton } from '@/components/SkeletonLoader'
import { cn } from "@/lib/utils";
import InstalledIconRow from '@/components/extensions/InstalledIconRow'

/**
 * Featured tab — curated list of verified MCP vendors Shannon ships with.
 *
 * Wire-up:
 * - Loads the static featured list from `list_featured_vendors` Rust command.
 * - OAuth vendors get a one-click "Connect" button → `install_mcp_oauth_loopback`
 *   binds an ephemeral loopback port, opens the browser, accepts the OAuth
 *   callback, exchanges the code (PKCE), and writes the MCP server config.
 *   The await resolves only after the whole flow finishes.
 * - stdio vendors (e.g. filesystem) install directly via `install_mcp_stdio`.
 *
 * Manual token paste is kept as a fallback for headless / browser-blocked
 * environments: if the loopback flow throws, the catch handler reveals the
 * `TokenPasteForm` so the user can paste an access token obtained out-of-band.
 */
export default function Featured() {
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })

  const { search } = useOutletContext<{ search: string }>();
  const [vendors, setVendors] = useState<FeaturedVendor[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [feedback, setFeedback] = useState<{ slug: string; msg: string; ok: boolean } | null>(null);
  const [tokenPrompt, setTokenPrompt] = useState<string | null>(null);
  // Batch E4: 公开（精选目录）/ 个人（本机已装资产） market dichotomy.
  const [marketTab, setMarketTab] = useState<'public' | 'personal'>('public');
  const [installed, setInstalled] = useState<InstalledAddonSummary[]>([]);

  useEffect(() => {
    let cancelled = false;
    const load = () => {
      listInstalledAddons()
        .then(rows => { if (!cancelled) setInstalled(rows) })
        .catch(() => { /* personal tab is opportunistic */ });
    };
    load();
    window.addEventListener('shannon:extension-installed', load);
    return () => {
      cancelled = true;
      window.removeEventListener('shannon:extension-installed', load);
    };
  }, []);

  const personalFiltered = search
    ? installed.filter(
        (a) =>
          a.name.toLowerCase().includes(search.toLowerCase()) ||
          a.id.toLowerCase().includes(search.toLowerCase())
      )
    : installed;

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    listFeaturedVendors()
      .then((rows) => {
        if (!cancelled) {
          setVendors(rows);
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

  async function handleConnect(vendor: FeaturedVendor) {
    setBusy(vendor.slug);
    setFeedback(null);
    try {
      if (vendor.install_kind.type === "oauth_remote") {
        await installMcpOAuthLoopback(vendor.slug);
        setFeedback({ slug: vendor.slug, msg: t('extensions.featured.connected'), ok: true });
      } else {
        // stdio featured vendor — install directly.
        const env: Record<string, string> = {};
        for (const [k, v] of vendor.install_kind.env_vars) env[k] = v;
        await installMcpStdio({
          server_name: vendor.slug,
          command: vendor.install_kind.command,
          args: vendor.install_kind.args,
          env: Object.entries(env),
        });
        setFeedback({ slug: vendor.slug, msg: t('extensions.featured.installed'), ok: true });
      }
    } catch (err) {
      console.error('[Featured] install failed:', err);
      setFeedback({ slug: vendor.slug, msg: t('extensions.featured.error.installFailed'), ok: false });
      if (vendor.install_kind.type === "oauth_remote") {
        // Loopback flow failed (port bind, browser launch, callback timeout,
        // token exchange, etc.). Reveal the manual paste form as a fallback
        // so users on headless / browser-blocked setups can still connect.
        setTokenPrompt(vendor.slug);
      }
    } finally {
      setBusy(null);
    }
  }

  async function handleSubmitToken(vendor: FeaturedVendor, token: string) {
    setBusy(vendor.slug);
    try {
      await installMcpOAuthComplete(vendor.slug, token);
      setFeedback({ slug: vendor.slug, msg: t('extensions.featured.connected'), ok: true });
      setTokenPrompt(null);
    } catch (err) {
      console.error('[Featured] oauth complete failed:', err);
      setFeedback({ slug: vendor.slug, msg: t('extensions.featured.error.connectFailed'), ok: false });
    } finally {
      setBusy(null);
    }
  }

  const filtered = search
    ? vendors.filter(
        (v) =>
          v.display_name.toLowerCase().includes(search.toLowerCase()) ||
          v.description.toLowerCase().includes(search.toLowerCase()) ||
          v.slug.toLowerCase().includes(search.toLowerCase())
      )
    : vendors;

  if (loading) {
    // Audit §P3-3 (round 6): align loading affordance with Triage's
    // CardSkeleton so the loading shimmer feels consistent across
    // collection pages.
    return (
      <div className="p-lg max-w-7xl mx-auto">
        <div className="mb-xl">
          <h2 className="text-headline-md font-headline-md text-on-surface mb-xs">{t('extensions.featured.title')}</h2>
          <p className="text-body-md text-on-surface-variant">{t('extensions.featured.subtitle')}</p>
        </div>
        <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-md">
          {Array.from({ length: 6 }).map((_, i) => <CardSkeleton key={i} />)}
        </div>
        <span className="sr-only">{t('extensions.featured.loading')}</span>
      </div>
    );
  }

  if (error) {
    return (
      <div className="p-lg max-w-5xl mx-auto">
        <div className="text-center py-3xl text-error">{t('extensions.featured.loadError')}: {error}</div>
      </div>
    );
  }

  return (
    <div className="p-lg max-w-7xl mx-auto">
      <div className="mb-md">
        <h2 className="text-headline-md font-headline-md text-on-surface mb-xs">{t('extensions.featured.title')}</h2>
        <p className="text-body-md text-on-surface-variant">
          {t('extensions.featured.subtitle')}
        </p>
      </div>

      {/* Batch E3: 已安装 icon row — installed assets surface on the hub's
          first screen, one click from the full inventory. */}
      <div className="mb-md">
        <InstalledIconRow />
      </div>

      {/* Batch E4: 公开 / 个人 market tabs (ZCode 插件市场 pattern). */}
      <div className="mb-lg">
        <div role="group" aria-label={t('extensions.market.tabs.aria')} className="inline-flex items-center rounded-lg bg-surface-container-low p-0.5">
          {([
            { id: 'public' as const, label: t('extensions.market.public') },
            { id: 'personal' as const, label: t('extensions.market.personal') },
          ]).map(opt => (
            <button
              key={opt.id}
              type="button"
              aria-pressed={marketTab === opt.id}
              onClick={() => setMarketTab(opt.id)}
              className={cn(
                'px-md py-xs rounded-md font-label-md text-label-md transition-colors cursor-pointer whitespace-nowrap',
                marketTab === opt.id
                  ? 'bg-primary text-on-primary shadow-sm font-bold'
                  : 'text-on-surface-variant hover:text-primary',
              )}
            >
              {opt.label}
            </button>
          ))}
        </div>
      </div>

      {marketTab === 'personal' ? (
        personalFiltered.length === 0 ? (
          <div className="border border-dashed border-outline-variant/40 rounded-2xl p-xl text-center">
            <span className="material-symbols-outlined icon-md text-on-surface-variant/60" aria-hidden="true">folder_off</span>
            <p className="font-label-md text-on-surface-variant mt-xs">{t('extensions.market.personalEmpty')}</p>
            <p className="font-label-sm text-on-surface-variant/70 mt-xs">{t('extensions.market.personalEmptyHint')}</p>
          </div>
        ) : (
          <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-lg">
            {personalFiltered.map(a => (
              <div
                key={a.id}
                className="rounded-2xl border border-outline-variant/30 bg-surface-container-lowest p-lg flex items-start gap-md hover:border-primary/40 transition-colors"
              >
                <div className={cn(
                  'w-11 h-11 rounded-xl flex items-center justify-center shrink-0',
                  a.enabled ? 'bg-primary/10 text-primary' : 'bg-surface-container-low text-on-surface-variant/60',
                )}>
                  <span className="material-symbols-outlined text-[22px]" aria-hidden="true">extension</span>
                </div>
                <div className="flex-1 min-w-0">
                  <div className="flex items-center gap-sm">
                    <h3 className="font-bold text-label-md text-on-surface truncate">{a.name}</h3>
                    <span className="font-label-xs px-xs py-[1px] rounded bg-surface-container-low text-on-surface-variant shrink-0">{a.kind}</span>
                  </div>
                  <p className="text-label-xs text-on-surface-variant font-mono truncate mt-[2px]">{a.id}</p>
                  {!a.enabled && (
                    <p className="text-label-xs text-warning mt-xs">{t('extensions.installed.disabled')}</p>
                  )}
                </div>
              </div>
            ))}
          </div>
        )
      ) : (
      <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-lg">
        {filtered.map((vendor) => {
          const isBusy = busy === vendor.slug;
          const feedbackForVendor = feedback?.slug === vendor.slug ? feedback : null;
          const showTokenPrompt = tokenPrompt === vendor.slug;
          const accent = ACCENT_BY_SLUG[vendor.slug] ?? ACCENT_DEFAULT;
          return (
            <div
              key={vendor.slug}
              className={`relative overflow-hidden rounded-3xl border border-outline-variant/30 bg-surface-container-lowest hover:border-primary/40 hover:shadow-xl hover:-translate-y-1 transition-all duration-200 flex flex-col group`}
            >
              {/* Accent strip */}
              <div className={cn("h-1.5 w-full bg-gradient-to-r", accent.bar)} />

              <div className="p-lg flex flex-col flex-1">
                <div className="flex items-start justify-between mb-md">
                  <div className={cn("relative w-14 h-14 rounded-2xl bg-gradient-to-br flex items-center justify-center shadow-md", accent.icon)}>
                    <span className="material-symbols-outlined text-white text-[28px] drop-shadow-sm max-w-full overflow-hidden">
                      {vendor.icon}
                    </span>
                  </div>
                  <TrustBadge trust={vendor.trust} />
                </div>

                <h3 className="font-bold text-label-lg text-on-surface mb-xs leading-tight">
                  {vendor.display_name}
                </h3>
                <p className="text-label-sm text-on-surface-variant flex-1 mb-sm leading-relaxed min-h-[40px]">
                  {vendor.description}
                </p>

                {/* Shannon installs every connector through the prompt-injection
                    scanner + signature verifier — surface it as a visible
                    differentiator (audit §3.6: the capability existed but was
                    never shown). */}
                <p className="text-[11px] text-on-surface-variant/90 mb-lg inline-flex items-center gap-1">
                  <span className="material-symbols-outlined text-[14px] text-success" aria-hidden="true">verified_user</span>
                  {t('extensions.featured.securityBadge')}
                </p>

                {showTokenPrompt && (
                  <TokenPasteForm
                    onSubmit={(token) => handleSubmitToken(vendor, token)}
                    onCancel={() => setTokenPrompt(null)}
                    disabled={isBusy}
                  />
                )}

                {feedbackForVendor && (
                  <div
                    className={cn(
                      "text-label-sm mb-sm inline-flex items-center gap-xs px-sm py-xs rounded-lg",
                      feedbackForVendor.ok
                        ? "bg-primary-container text-on-primary-container"
                        : "bg-error-container/50 text-on-error-container",
                    )}
                  >
                    <span className="material-symbols-outlined text-[14px]">
                      {feedbackForVendor.ok ? "check_circle" : "error"}
                    </span>
                    {feedbackForVendor.msg}
                  </div>
                )}

                {!showTokenPrompt && (
                  // 2026-09 axe-ci: bypass <Button> + cva here. shadcn base's
                  // `disabled:opacity-50` lingers in the cascade even after
                  // we stripped it (variant classList ordering keeps the
                  // muted look on primary bg in some themes). A native
                  // <button> with className composed inline gives us full
                  // control over the disabled style, and axe verifies the
                  // final computed style.
                  <button
                    type="button"
                    onClick={() => handleConnect(vendor)}
                    disabled={isBusy}
                    aria-busy={isBusy || undefined}
                    className="group/button inline-flex shrink-0 items-center justify-center rounded-xl border border-transparent bg-primary text-on-primary text-label-md font-bold shadow-sm hover:shadow-md w-full px-md py-sm transition-all outline-none select-none focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50 disabled:cursor-not-allowed disabled:bg-surface-container disabled:text-on-surface disabled:shadow-none"
                  >
                    {isBusy ? (
                      <>
                        <Spinner className="icon-sm" />
                        {vendor.install_kind.type === "oauth_remote"
                          ? t('extensions.featured.authorizing')
                          : t('extensions.featured.installing')}
                      </>
                    ) : vendor.install_kind.type === "oauth_remote" ? (
                      <>
                        <span className="material-symbols-outlined text-[18px]">link</span>
                        {t('extensions.featured.connect')}
                      </>
                    ) : (
                      <>
                        <span className="material-symbols-outlined text-[18px]">download</span>
                        {t('extensions.featured.install')}
                      </>
                    )}
                  </button>
                )}
              </div>
            </div>
          );
        })}
      </div>
      )}

      {marketTab === 'public' && filtered.length === 0 && (
        <div className="text-center py-3xl text-on-surface-variant">
          {search ? intl.formatMessage({ id: 'extensions.featured.noMatches' }, { search }) : t('extensions.featured.noVendors')}
        </div>
      )}
    </div>
  );
}

/// Per-vendor accent palettes. Each entry picks a coherent gradient for the
/// top accent strip, icon halo, and CTA button. The default is a neutral
/// Shannon brand gradient. Keep the palette names stable so designers can
/// re-skin in one place.
const ACCENT_DEFAULT = {
  bar: "from-primary/60 to-primary/20",
  icon: "from-primary to-primary/70",
  button: "from-primary to-primary/80",
};
const ACCENT_BY_SLUG: Record<string, typeof ACCENT_DEFAULT> = {
  github: { bar: "from-slate-600/60 to-slate-400/20", icon: "from-slate-700 to-slate-500", button: "from-slate-700 to-slate-600" },
  gitlab: { bar: "from-orange-500/60 to-amber-400/20", icon: "from-orange-600 to-amber-500", button: "from-orange-600 to-amber-600" },
  linear: { bar: "from-indigo-500/60 to-violet-400/20", icon: "from-indigo-600 to-violet-500", button: "from-indigo-600 to-violet-600" },
  notion: { bar: "from-zinc-800/60 to-zinc-500/20", icon: "from-zinc-800 to-zinc-600", button: "from-zinc-800 to-zinc-700" },
  slack: { bar: "from-purple-500/60 to-rose-400/20", icon: "from-purple-600 to-rose-500", button: "from-purple-600 to-rose-600" },
  figma: { bar: "from-pink-500/60 to-orange-400/20", icon: "from-pink-600 to-orange-500", button: "from-pink-600 to-orange-600" },
  filesystem: { bar: "from-emerald-500/60 to-teal-400/20", icon: "from-emerald-600 to-teal-500", button: "from-emerald-600 to-teal-600" },
  postgres: { bar: "from-blue-500/60 to-sky-400/20", icon: "from-blue-600 to-sky-500", button: "from-blue-600 to-sky-600" },
};

function TrustBadge({ trust }: { trust: FeaturedVendor["trust"] }) {
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })

  const labels: Record<FeaturedVendor["trust"], { text: string; cls: string }> = {
    verified: { text: t('extensions.featured.trust.verified'), cls: "bg-primary-container text-on-primary-container" },
    official: { text: t('extensions.featured.trust.official'), cls: "bg-primary text-on-primary" },
    community: { text: t('extensions.featured.trust.community'), cls: "bg-tertiary-container/50 text-on-tertiary-container" },
    unknown: { text: t('extensions.featured.trust.unknown'), cls: "bg-surface-container-highest text-on-surface-variant" },
  };
  const { text, cls } = labels[trust];
  return (
    <span className={cn("text-label-xs px-sm py-[2px] rounded-full font-bold", cls)}>{text}</span>
  );
}

function TokenPasteForm({
  onSubmit,
  onCancel,
  disabled,
}: {
  onSubmit: (token: string) => void;
  onCancel: () => void;
  disabled: boolean;
}) {
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })

  const [token, setToken] = useState("");
  return (
    <div className="mb-sm">
      <p className="text-label-xs text-on-surface-variant mb-xs">
        {t('extensions.featured.tokenPrompt')}
      </p>
      <input
        type="password"
        value={token}
        onChange={(e) => setToken(e.target.value)}
        placeholder={t('extensions.featured.tokenPlaceholder')}
        className="w-full px-sm py-xs rounded border border-outline-variant text-label-sm bg-surface mb-xs"
        disabled={disabled}
      />
      <div className="flex gap-xs">
        <Button
          type="button"
          size="sm"
          onClick={() => token && onSubmit(token)}
          disabled={disabled || !token}
          className="flex-1 rounded"
        >
          {t('extensions.featured.tokenSubmit')}
        </Button>
        <Button
          variant="secondary"
          size="sm"
          type="button"
          onClick={onCancel}
          disabled={disabled}
          className="rounded"
        >
          {t('extensions.featured.tokenCancel')}
        </Button>
      </div>
    </div>
  );
}
