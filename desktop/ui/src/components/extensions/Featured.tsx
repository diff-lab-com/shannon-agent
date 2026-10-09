import { useEffect, useState } from "react";
import { Spinner } from '@/components/ui/loading-state'
import { useOutletContext, useNavigate } from "react-router-dom";
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
import { SecurityBadge } from '@/components/extensions/SecurityBadge'

// Batch E4/B3 P1-21 contract: any successful install from this page must
// dispatch `shannon:extension-installed` (same shape InstallDialog uses) so
// the other extension tabs (Installed, InstalledIconRow, the personal tab
// below) refresh immediately instead of showing stale inventories.
function announceInstalled(name: string) {
  window.dispatchEvent(
    new CustomEvent('shannon:extension-installed', {
      detail: { kind: 'mcp', name },
    }),
  )
}

/**
 * Featured tab — curated list of verified MCP vendors Shannon ships with.
 *
 * Wire-up:
 * - Loads the static featured list from `list_featured_vendors` Rust command.
 * - OAuth vendors get a one-click "Add" button → `install_mcp_oauth_loopback`
 *   binds an ephemeral loopback port, opens the browser, accepts the OAuth
 *   callback, exchanges the code (PKCE), and writes the MCP server config.
 *   The await resolves only after the whole flow finishes.
 * - stdio vendors (e.g. filesystem) install directly via `install_mcp_stdio`.
 * - F-11 unified card anatomy: foot = security badge + ONE primary action —
 *   「添加」 when the vendor is not installed (matched against
 *   list_installed_addons), 「管理」 (→ /extensions/mcp-servers) when it is.
 *   No connected/needs-auth claim is ever rendered: the backend exposes no
 *   MCP connection-state API, and honesty beats decoration.
 *
 * Manual token paste is kept as a fallback for headless / browser-blocked
 * environments: if the loopback flow throws, the catch handler reveals the
 * `TokenPasteForm` so the user can paste an access token obtained out-of-band.
 */
export default function Featured() {
  const intl = useIntl()
  const navigate = useNavigate()
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
        announceInstalled(vendor.slug);
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
        announceInstalled(vendor.slug);
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
      announceInstalled(vendor.slug);
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

  // F-11: the honest per-card state bit. The aggregator names MCP rows after
  // their config key and installs use `server_name: vendor.slug`, so a slug
  // hit means this exact vendor is on the machine (refreshes via the
  // `shannon:extension-installed` listener above).
  const installedMcpNames = new Set(
    installed.filter((a) => a.kind === 'mcp').map((a) => a.name),
  );

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
                  ? 'bg-primary text-on-primary shadow-e1 font-bold'
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
            {search ? (
              // B3 (P2 顺带): a no-match search must not read as "nothing
              // installed" — same distinction the public tab makes.
              <>
                <span className="material-symbols-outlined icon-md text-on-surface-variant/60" aria-hidden="true">search_off</span>
                <p className="font-label-md text-on-surface-variant mt-xs">
                  {intl.formatMessage({ id: 'extensions.market.personalNoMatches' }, { search })}
                </p>
              </>
            ) : (
              <>
                <span className="material-symbols-outlined icon-md text-on-surface-variant/60" aria-hidden="true">folder_off</span>
                <p className="font-label-md text-on-surface-variant mt-xs">{t('extensions.market.personalEmpty')}</p>
                <p className="font-label-sm text-on-surface-variant/70 mt-xs">{t('extensions.market.personalEmptyHint')}</p>
              </>
            )}
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
                  a.enabled ? 'bg-primary-container text-on-primary-container' : 'bg-surface-container-low text-on-surface-variant/60',
                )}>
                  <span className="material-symbols-outlined icon-lg" aria-hidden="true">extension</span>
                </div>
                <div className="flex-1 min-w-0">
                  <div className="flex items-center gap-sm">
                    <h3 className="font-bold text-label-md text-on-surface truncate">{a.name}</h3>
                    <span className="font-label-xs px-xs py-[1px] rounded-sm bg-surface-container-low text-on-surface-variant shrink-0">{a.kind}</span>
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
          // F-11 (honest states): the only per-vendor bit the backend gives us
          // beyond the catalog row itself is "installed" — MCP rows in
          // list_installed_addons carry the server name, and every featured
          // install uses `server_name: vendor.slug`. There is NO
          // connected/needs-auth API, so the card never claims 已连接/去认证;
          // an uninstalled OAuth vendor simply shows 「添加」 (the install IS
          // the auth flow).
          const isInstalled = installedMcpNames.has(vendor.slug);
          return (
            <div
              key={vendor.slug}
              className={`relative overflow-hidden rounded-3xl border border-outline-variant/30 bg-surface-container-lowest hover:border-primary/40 hover:shadow-e4 hover:-translate-y-1 transition-all duration-(--duration-normal) flex flex-col group`}
            >
              {/* Accent strip */}
              <div className={cn("h-1.5 w-full bg-gradient-to-r", accent.bar)} />

              <div className="p-lg flex flex-col flex-1">
                <div className="flex items-start justify-between mb-md gap-sm">
                  <div className={cn("relative w-14 h-14 rounded-2xl bg-gradient-to-br flex items-center justify-center shadow-e2", accent.icon)}>
                    <span className="material-symbols-outlined text-white icon-xl drop-shadow-e1 max-w-full overflow-hidden">
                      {vendor.icon}
                    </span>
                  </div>
                  {/* Status badge slot (F-11 top row): real trust level only. */}
                  <TrustBadge trust={vendor.trust} />
                </div>

                <h3 className="font-bold text-label-lg text-on-surface mb-xs leading-tight">
                  {vendor.display_name}
                </h3>
                <p className="text-label-sm text-on-surface-variant flex-1 mb-sm leading-relaxed min-h-[40px]">
                  {vendor.description}
                </p>

                {feedbackForVendor && (
                  <div
                    className={cn(
                      "text-label-sm mb-sm inline-flex items-center gap-xs px-sm py-xs rounded-lg",
                      feedbackForVendor.ok
                        ? "bg-primary-container text-on-primary-container"
                        : "bg-error-container/50 text-on-error-container",
                    )}
                  >
                    <span className="material-symbols-outlined icon-sm">
                      {feedbackForVendor.ok ? "check_circle" : "error"}
                    </span>
                    {feedbackForVendor.msg}
                  </div>
                )}

                {showTokenPrompt && (
                  <TokenPasteForm
                    onSubmit={(token) => handleSubmitToken(vendor, token)}
                    onCancel={() => setTokenPrompt(null)}
                    disabled={isBusy}
                  />
                )}

                {/* F-11 unified card foot: security badge left, ONE primary
                    action right. The SecurityBadge shows the scan's verdict —
                    when it has nothing to warn about the card's standing
                    indicator line fills the same slot, so every card carries a
                    visible security marker (design 07-connectors foot).
                    缓期批 2 honesty fix: the line used to read「Injection-
                    scanned · Signed」, overstating the machinery on both
                    counts — the scan is an install-time advisory pass over
                    the directory and README (~22 static patterns; runtime
                    outbound content is never scanned), and "signed" is a
                    self-declared manifest label, not cryptographic
                    verification (extensions/security.rs says so itself).
                    Same slot, same tone; the copy + hover helper text now
                    say what actually exists. */}
                <div className="mt-auto pt-sm flex items-center gap-sm min-w-0">
                  <SecurityBadge
                    text={vendor.description}
                    trust={vendor.trust}
                    fallback={
                      <span
                        className="text-label-xs text-on-surface-variant/90 inline-flex items-center gap-xs min-w-0"
                        title={`${t('extensions.featured.securityBadgeScanHelp')} ${t('extensions.featured.securityBadgePublisherHelp')}`}
                      >
                        <span className="material-symbols-outlined icon-sm text-success shrink-0" aria-hidden="true">verified_user</span>
                        <span className="truncate">{t('extensions.featured.securityBadge')}</span>
                      </span>
                    }
                  />
                  <span className="flex-1" aria-hidden="true" />
                  {!showTokenPrompt && (
                    // 2026-09 axe-ci: bypass <Button> + cva here. shadcn base's
                    // `disabled:opacity-50` lingers in the cascade even after
                    // we stripped it (variant classList ordering keeps the
                    // muted look on primary bg in some themes). A native
                    // <button> with className composed inline gives us full
                    // control over the disabled style, and axe verifies the
                    // final computed style.
                    // B4 (F-11): unified soft-primary form (design `.btn.soft`)
                    // for both states — 未安装→添加, 已安装→管理.
                    <button
                      type="button"
                      onClick={() =>
                        isInstalled
                          ? navigate('/extensions/mcp-servers')
                          : handleConnect(vendor)
                      }
                      disabled={isBusy}
                      aria-busy={isBusy || undefined}
                      data-testid={`featured-action-${vendor.slug}`}
                      className="group/button inline-flex shrink-0 items-center justify-center gap-xs rounded-xl border border-primary/30 bg-primary-container text-on-primary-container text-label-md font-bold px-md py-xs transition-all outline-none select-none focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50 disabled:cursor-not-allowed disabled:bg-surface-container disabled:text-on-surface disabled:border-transparent"
                    >
                      {isBusy ? (
                        <>
                          <Spinner className="icon-sm" />
                          {vendor.install_kind.type === "oauth_remote"
                            ? t('extensions.featured.authorizing')
                            : t('extensions.featured.installing')}
                        </>
                      ) : isInstalled ? (
                        <>
                          <span className="material-symbols-outlined icon-md" aria-hidden="true">arrow_forward</span>
                          {t('extensions.featured.manage')}
                        </>
                      ) : (
                        <>
                          <span className="material-symbols-outlined icon-md" aria-hidden="true">add</span>
                          {t('extensions.featured.add')}
                        </>
                      )}
                    </button>
                  )}
                </div>
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
        className="w-full px-sm py-xs rounded-sm border border-outline-variant text-label-sm bg-surface mb-xs"
        disabled={disabled}
      />
      <div className="flex gap-xs">
        <Button
          type="button"
          size="sm"
          onClick={() => token && onSubmit(token)}
          disabled={disabled || !token}
          className="flex-1 rounded-sm"
        >
          {t('extensions.featured.tokenSubmit')}
        </Button>
        <Button
          variant="secondary"
          size="sm"
          type="button"
          onClick={onCancel}
          disabled={disabled}
          className="rounded-sm"
        >
          {t('extensions.featured.tokenCancel')}
        </Button>
      </div>
    </div>
  );
}
