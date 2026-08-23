// Tab 1: Search the registry
//
// Debounced search over the MCP registry. One-click install via
// buildSpecFromPackage + installMcpStdio.

import { useEffect, useMemo, useState } from "react";
import { useIntl } from "react-intl";
import { toast } from "sonner";
import {
  listMcpRegistryServers,
  installMcpStdio,
  type RegistryServer,
} from "@/lib/tauri-api";
import { safeErrorMessage } from "@/lib/packageValidation";
import LoadingState from "@/components/ui/loading-state";
import { Button } from "@/components/ui/button";
import {
  buildSpecFromPackage,
  packageManagerLabel,
} from "./utils";
import type { RegistryServerWithPackage } from "./types";

export function SearchTab({
  installedNames,
  initialQuery,
  onInstalled,
}: {
  installedNames: Set<string>;
  initialQuery: string;
  onInstalled: () => void;
}) {
  const intl = useIntl();
  const t = (id: string, values?: Record<string, string | number>) =>
    intl.formatMessage({ id }, values);

  const [query, setQuery] = useState(initialQuery);
  const [registry, setRegistry] = useState<RegistryServer[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [busyId, setBusyId] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    listMcpRegistryServers()
      .then((rows) => {
        if (!cancelled) {
          setRegistry(rows);
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

  // Debounce the query: 250ms after the last keystroke.
  const [debounced, setDebounced] = useState(query);
  useEffect(() => {
    const h = setTimeout(() => setDebounced(query), 250);
    return () => clearTimeout(h);
  }, [query]);

  const filtered = useMemo(() => {
    const q = debounced.trim().toLowerCase();
    if (!q) return registry;
    return registry.filter(
      (s) =>
        s.name.toLowerCase().includes(q) ||
        (s.description ?? "").toLowerCase().includes(q),
    );
  }, [registry, debounced]);

  async function handleInstall(server: RegistryServer) {
    const serverWithPkg = server as RegistryServerWithPackage;
    const spec = buildSpecFromPackage(server.name, serverWithPkg.package);
    if (!spec) {
      toast.error(t("extensions.mcp.oneClick.noPackageMetadata"));
      return;
    }
    setBusyId(server.id);
    try {
      await installMcpStdio(spec);
      toast.success(
        t("extensions.mcp.oneClick.installSuccess", { name: server.name }),
      );
      onInstalled();
    } catch (err) {
      toast.error(
        t("extensions.mcp.oneClick.installFailed", {
          error: safeErrorMessage(err, "install failed"),
        }),
      );
    } finally {
      setBusyId(null);
    }
  }

  return (
    <div className="flex flex-col gap-sm" role="tabpanel">
      <input
        type="text"
        value={query}
        onChange={(e) => setQuery(e.target.value)}
        placeholder={t("extensions.mcp.addDialog.search.placeholder")}
        aria-label={t("extensions.mcp.addDialog.search.placeholder")}
        className="w-full px-md py-sm rounded-lg border border-outline-variant/40 bg-surface text-label-sm focus:outline-none focus-visible:ring-2 focus-visible:ring-primary/30"
        autoFocus
      />

      {loading && (
        <LoadingState size="sm" label={t("extensions.mcp.fetching")} />
      )}

      {error && (
        <div className="border border-error/30 rounded-xl p-md bg-error-container/10 text-label-sm text-error flex items-start gap-sm">
          <span className="material-symbols-outlined text-error text-[18px] shrink-0">error</span>
          <span>
            {t("extensions.mcp.registryError")}{" "}
            <span className="font-mono">{error}</span>
          </span>
        </div>
      )}

      {!loading && !error && filtered.length === 0 && (
        <div className="text-center py-md text-on-surface-variant text-label-sm">
          {t("extensions.mcp.addDialog.search.empty")}
        </div>
      )}

      {!loading && !error && filtered.length > 0 && (
        <div className="max-h-[50vh] overflow-y-auto flex flex-col gap-xs">
          {filtered.map((server) => {
            const isBusy = busyId === server.id;
            const isInstalled = installedNames.has(server.name);
            const pkgManager = packageManagerLabel(
              (server as RegistryServerWithPackage).package,
            );
            return (
              <div
                key={server.id}
                className="border border-outline-variant/30 rounded-xl p-sm bg-surface-container-low/40 flex items-start gap-sm"
              >
                <div className="flex-1 min-w-0">
                  <div className="flex items-center gap-xs">
                    <span className="font-bold text-label-md text-on-surface truncate">
                      {server.name}
                    </span>
                    {server.verified && (
                      <span className="text-label-xs px-xs py-[1px] rounded-full bg-primary-container/60 text-on-primary-container font-bold">
                        {t("extensions.mcp.verified")}
                      </span>
                    )}
                    {isInstalled && (
                      <span className="text-label-xs px-xs py-[1px] rounded-full bg-secondary-container/60 text-on-secondary-container font-bold">
                        {t("extensions.mcp.installed")}
                      </span>
                    )}
                  </div>
                  {server.description && (
                    <p className="text-label-xs text-on-surface-variant line-clamp-2 mt-[2px]">
                      {server.description}
                    </p>
                  )}
                  {pkgManager && !isInstalled && (
                    <p className="text-label-xs text-on-surface-variant mt-[2px]">
                      {t("extensions.mcp.oneClick.autoInstallHint", {
                        packageManager: pkgManager,
                      })}
                    </p>
                  )}
                </div>
                <Button
                  type="button"
                  size="sm"
                  onClick={() => handleInstall(server)}
                  disabled={isBusy || isInstalled}
                  className="shrink-0 disabled:cursor-not-allowed"
                >
                  {isBusy
                    ? "…"
                    : isInstalled
                      ? t("extensions.mcp.installed")
                      : t("extensions.mcp.install")}
                </Button>
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}