import { useEffect, useState } from "react";
import { useOutletContext } from "react-router-dom";
import {
  listFeaturedVendors,
  installMcpOAuthAuthorizeUrl,
  installMcpOAuthComplete,
  installMcpStdio,
  type FeaturedVendor,
} from "@/lib/tauri-api";

/**
 * Featured tab — curated list of verified MCP vendors Shannon ships with.
 *
 * P2 wire-up:
 * - Loads the static featured list from `list_featured_vendors` Rust command.
 * - OAuth vendors get a one-click "Connect" button → opens browser via
 *   `install_mcp_oauth_authorize_url` → user authorizes → paste the code or
 *   the loopback listener hands back the token → `install_mcp_oauth_complete`.
 * - stdio vendors (e.g. filesystem) install directly via `install_mcp_stdio`.
 *
 * For P2 the OAuth flow is simplified: we open the URL and show a "Paste your
 * access token" prompt. The loopback listener path is wired in the Rust layer
 * but requires a desktop process event channel to surface back to the UI.
 */
export default function Featured() {
  const { search } = useOutletContext<{ search: string }>();
  const [vendors, setVendors] = useState<FeaturedVendor[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [feedback, setFeedback] = useState<{ slug: string; msg: string; ok: boolean } | null>(null);
  const [tokenPrompt, setTokenPrompt] = useState<string | null>(null);

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
        const redirectUri = "http://localhost:1738/callback";
        const { url } = await installMcpOAuthAuthorizeUrl(vendor.slug, redirectUri);
        // Open in default browser. In Tauri, window.open works for external URLs.
        window.open(url, "_blank", "noopener,noreferrer");
        setTokenPrompt(vendor.slug);
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
        setFeedback({ slug: vendor.slug, msg: "Installed", ok: true });
      }
    } catch (err) {
      setFeedback({ slug: vendor.slug, msg: String(err), ok: false });
    } finally {
      setBusy(null);
    }
  }

  async function handleSubmitToken(vendor: FeaturedVendor, token: string) {
    setBusy(vendor.slug);
    try {
      await installMcpOAuthComplete(vendor.slug, token);
      setFeedback({ slug: vendor.slug, msg: "Connected", ok: true });
      setTokenPrompt(null);
    } catch (err) {
      setFeedback({ slug: vendor.slug, msg: String(err), ok: false });
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
    return (
      <div className="p-lg max-w-5xl mx-auto">
        <div className="text-center py-3xl text-on-surface-variant">Loading featured vendors…</div>
      </div>
    );
  }

  if (error) {
    return (
      <div className="p-lg max-w-5xl mx-auto">
        <div className="text-center py-3xl text-error">Failed to load: {error}</div>
      </div>
    );
  }

  return (
    <div className="p-lg max-w-5xl mx-auto">
      <div className="mb-xl">
        <h2 className="text-headline-md font-bold text-on-surface mb-xs">Featured Extensions</h2>
        <p className="text-body-md text-on-surface-variant">
          Curated MCP servers verified by Shannon. Click Connect to authorize via OAuth.
        </p>
      </div>

      <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-md">
        {filtered.map((vendor) => {
          const isBusy = busy === vendor.slug;
          const feedbackForVendor = feedback?.slug === vendor.slug ? feedback : null;
          const showTokenPrompt = tokenPrompt === vendor.slug;
          return (
            <div
              key={vendor.slug}
              className="border border-outline-variant/30 rounded-2xl p-lg bg-surface-container-low/50 hover:bg-surface-container-low transition-colors flex flex-col"
            >
              <div className="flex items-start justify-between mb-sm">
                <div className="w-10 h-10 rounded-lg bg-primary/10 flex items-center justify-center">
                  <span className="material-symbols-outlined text-primary text-[20px]">
                    {vendor.icon}
                  </span>
                </div>
                <TrustBadge trust={vendor.trust} />
              </div>
              <h3 className="font-bold text-label-md text-on-surface mb-xs">
                {vendor.display_name}
              </h3>
              <p className="text-label-sm text-on-surface-variant flex-1 mb-md">
                {vendor.description}
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
                  className={`text-label-sm mb-sm ${
                    feedbackForVendor.ok ? "text-primary" : "text-error"
                  }`}
                >
                  {feedbackForVendor.msg}
                </div>
              )}

              {!showTokenPrompt && (
                <button
                  type="button"
                  onClick={() => handleConnect(vendor)}
                  disabled={isBusy}
                  className="w-full inline-flex items-center justify-center gap-xs px-md py-sm rounded-lg bg-primary text-on-primary text-label-sm font-bold hover:bg-primary/90 disabled:opacity-50 disabled:cursor-not-allowed transition-colors"
                >
                  {isBusy ? (
                    <>
                      <span className="material-symbols-outlined text-[16px] animate-spin">progress_activity</span>
                      Working…
                    </>
                  ) : vendor.install_kind.type === "oauth_remote" ? (
                    <>
                      <span className="material-symbols-outlined text-[16px]">link</span>
                      Connect
                    </>
                  ) : (
                    <>
                      <span className="material-symbols-outlined text-[16px]">download</span>
                      Install
                    </>
                  )}
                </button>
              )}
            </div>
          );
        })}
      </div>

      {filtered.length === 0 && (
        <div className="text-center py-3xl text-on-surface-variant">
          {search ? `No matches for "${search}"` : "No featured vendors available."}
        </div>
      )}
    </div>
  );
}

function TrustBadge({ trust }: { trust: FeaturedVendor["trust"] }) {
  const labels: Record<FeaturedVendor["trust"], { text: string; cls: string }> = {
    verified: { text: "Verified", cls: "bg-primary-container/50 text-on-primary-container" },
    official: { text: "Official", cls: "bg-secondary-container/50 text-on-secondary-container" },
    community: { text: "Community", cls: "bg-tertiary-container/50 text-on-tertiary-container" },
    unknown: { text: "Unknown", cls: "bg-surface-container-highest text-on-surface-variant" },
  };
  const { text, cls } = labels[trust];
  return (
    <span className={`text-label-xs px-sm py-[2px] rounded-full font-bold ${cls}`}>{text}</span>
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
  const [token, setToken] = useState("");
  return (
    <div className="mb-sm">
      <p className="text-label-xs text-on-surface-variant mb-xs">
        After authorizing in your browser, paste the access token:
      </p>
      <input
        type="password"
        value={token}
        onChange={(e) => setToken(e.target.value)}
        placeholder="access_token"
        className="w-full px-sm py-xs rounded border border-outline-variant text-label-sm bg-surface mb-xs"
        disabled={disabled}
      />
      <div className="flex gap-xs">
        <button
          type="button"
          onClick={() => token && onSubmit(token)}
          disabled={disabled || !token}
          className="flex-1 px-sm py-xs rounded bg-primary text-on-primary text-label-xs font-bold disabled:opacity-50"
        >
          Submit
        </button>
        <button
          type="button"
          onClick={onCancel}
          disabled={disabled}
          className="px-sm py-xs rounded bg-surface-container-high text-on-surface text-label-xs font-bold"
        >
          Cancel
        </button>
      </div>
    </div>
  );
}
