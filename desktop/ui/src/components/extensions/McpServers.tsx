import { useEffect, useState } from "react";
import EmptyState from '@/components/ui/empty-state'
import ErrorState from '@/components/ui/error-state'
import { useNavigate, useOutletContext } from "react-router-dom";
import { useIntl } from "react-intl";
import { toast } from "sonner";
import {
  listMcpServers,
  restartMcpServer,
  uninstallMcpServer,
} from "@/lib/tauri-api";
import { safeErrorMessage } from "@/lib/packageValidation";
import type { McpServerInfo } from "@/types";
import McpAddServerDialog from "./McpAddServerDialog";
import { ConfirmDialog } from "@/components/ui/confirm-dialog";
import LoadingState from "@/components/ui/loading-state";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";

/** Semantic icon per known MCP server. Falls back to a hub/storage icon. */
const MCP_SERVER_ICONS: Record<string, string> = {
  filesystem: 'folder',
  fs: 'folder',
  github: 'hub',
  gitlab: 'hub',
  playwright: 'theater_comedy',
  puppeteer: 'web',
  sqlite: 'database',
  postgres: 'database',
  postgresql: 'database',
  mysql: 'database',
  redis: 'bolt',
  memory: 'psychology',
  fetch: 'cloud_download',
  slack: 'tag',
  linear: 'linear_scale',
  notion: 'description',
  obsidian: 'book',
  imap: 'mail',
  smtp: 'mail',
  brave: 'shield',
  google: 'travel_explore',
  sequential: 'route',
  time: 'schedule',
};

function mcpServerIcon(name: string): string {
  const key = name.toLowerCase().trim();
  for (const [k, v] of Object.entries(MCP_SERVER_ICONS)) {
    if (key.includes(k)) return v;
  }
  return 'cloud';
}

/**
 * MCP Servers page — Cursor-style click-install UX.
 *
 * Layout (top to bottom):
 *   1. Page header (title + subtitle).
 *   2. Installed servers section (each row: name + status pill + command
 *      preview + uninstall). Friendly empty state when none installed.
 *   3. "Add Server" CTA — primary button that opens the modal.
 *
 * The modal (`McpAddServerDialog`) hosts three tabs:
 *   - Search the MCP registry (one-click install).
 *   - Paste JSON (Cursor / Claude Desktop format) and bulk install.
 *   - Manual stdio form (name + command + args + env).
 *
 * All install business logic lives in the modal. This component owns the
 * `installed` list state and the refresh callback passed down.
 */
export default function McpServers() {
  const intl = useIntl();
  const t = (id: string, values?: Record<string, string | number>) =>
    intl.formatMessage({ id }, values);
  const navigate = useNavigate();

  // The Extensions shell pipes a shared search box down via outlet context.
  // We no longer render the registry inline, so the value is only consulted
  // when the user opens the modal (the modal reads it as the initial query).
  const { search } = useOutletContext<{ search: string }>();

  const [installed, setInstalled] = useState<McpServerInfo[]>([]);
  const [installedLoading, setInstalledLoading] = useState(true);
  // B3 P1-17: a failed list read must not render as "nothing installed".
  const [installedError, setInstalledError] = useState<string | null>(null);
  const [busyId, setBusyId] = useState<string | null>(null);
  const [dialogOpen, setDialogOpen] = useState(false);
  const [removeTarget, setRemoveTarget] = useState<string | null>(null);

  const refreshInstalled = () => {
    listMcpServers()
      .then((rows) => {
        setInstalled(rows);
        setInstalledError(null);
      })
      .catch((err) => {
        setInstalledError(err instanceof Error ? err.message : String(err));
      })
      .finally(() => setInstalledLoading(false));
  };

  useEffect(() => {
    refreshInstalled();
     
  }, []);

  async function handleUninstall(name: string) {
    setBusyId(`uninstall:${name}`);
    try {
      await uninstallMcpServer(name);
      toast.success(t("extensions.mcp.removed", { name }));
      refreshInstalled();
    } catch (err) {
      // B3 (P2 顺带): uninstall failures get their own message — reusing the
      // "install failed" copy told users the opposite of what happened.
      toast.error(
        intl.formatMessage(
          { id: "extensions.mcp.uninstallFailed" },
          { error: safeErrorMessage(err, "uninstall failed") },
        ),
      );
    } finally {
      setBusyId(null);
    }
  }

  // G1 P0-1.4 — per-server restart wired to the existing
  // `restart_mcp_server` backend (stop + start, tool list re-probed by the
  // refresh below).
  async function handleRestart(name: string) {
    setBusyId(`restart:${name}`);
    try {
      await restartMcpServer(name);
      toast.success(t("extensions.mcp.restarted", { name }));
    } catch (err) {
      toast.error(
        intl.formatMessage(
          { id: "extensions.mcp.restartFailed" },
          { error: safeErrorMessage(err, "restart failed") },
        ),
      );
    } finally {
      setBusyId(null);
      refreshInstalled();
    }
  }

  function handleInstalled() {
    refreshInstalled();
  }

  return (
    <div className="p-lg max-w-medium mx-auto space-y-xl">
      <header>
        <h2 className="text-headline-md font-headline-md text-on-surface mb-xs">
          {t("extensions.mcp.title")}
        </h2>
        <p className="text-body-md text-on-surface-variant">
          {t("extensions.mcp.subtitle")}
        </p>
      </header>

      <InstalledSection
        servers={installed}
        loading={installedLoading}
        error={installedError}
        onRetry={refreshInstalled}
        busyId={busyId}
        onUninstall={(name) => setRemoveTarget(name)}
        onRestart={handleRestart}
        onOpenPermissions={(name) =>
          // X3 权限就近直达 — deep link into the permissions page pre-filtered
          // to this server. The page matches rules against `mcp__<name>__*`;
          // the scope query carries the `mcp:<name>` form (URL-encoded).
          navigate(`/settings/permissions?scope=${encodeURIComponent(`mcp:${name}`)}`)
        }
      />

      <ConfirmDialog
        open={removeTarget !== null}
        title={t("extensions.mcp.removeConfirm.title")}
        message={t("extensions.mcp.removeConfirm.message", { name: removeTarget ?? "" })}
        confirmLabel={t("extensions.mcp.removeConfirm.confirm")}
        cancelLabel={t("extensions.mcp.removeConfirm.cancel")}
        destructive
        busy={busyId?.startsWith("uninstall:") ?? false}
        onConfirm={() => {
          if (removeTarget) void handleUninstall(removeTarget).finally(() => setRemoveTarget(null))
        }}
        onCancel={() => setRemoveTarget(null)}
      />

      <div className="flex justify-center pt-sm">
        <Button
          type="button"
          onClick={() => setDialogOpen(true)}
          className="px-lg py-sm rounded-xl hover:bg-primary/90 cursor-pointer"
        >
          <span className="material-symbols-outlined icon-md">add</span>
          {t("extensions.mcp.addDialog.cta")}
        </Button>
      </div>

      <McpAddServerDialog
        open={dialogOpen}
        onClose={() => setDialogOpen(false)}
        onInstalled={handleInstalled}
        installedNames={new Set(installed.map((s) => s.name))}
        initialQuery={search}
      />
    </div>
  );
}

// ---------------------------------------------------------------------------
// Installed servers (with uninstall) — shown at the TOP of the page
// ---------------------------------------------------------------------------

function InstalledSection({
  servers,
  loading,
  error,
  onRetry,
  busyId,
  onUninstall,
  onRestart,
  onOpenPermissions,
}: {
  servers: McpServerInfo[];
  loading: boolean;
  /** B3 P1-17: load failure — rendered as a distinct error state, never as
   *  the empty state. */
  error: string | null;
  onRetry: () => void;
  busyId: string | null;
  onUninstall: (name: string) => void;
  onRestart: (name: string) => void;
  onOpenPermissions: (name: string) => void;
}) {
  const intl = useIntl();
  const t = (id: string, values?: Record<string, string | number>) =>
    intl.formatMessage({ id }, values);

  return (
    <section>
      <h3 className="text-label-lg font-bold text-on-surface-variant uppercase tracking-wide mb-sm">
        {t("extensions.mcp.installedSection")} · {servers.length}
      </h3>
      {loading ? (
        <LoadingState size="sm" label={t("extensions.mcp.loading")} />
      ) : error ? (
        <div className="border border-outline-variant/30 rounded-2xl bg-surface-container-lowest/50">
          <ErrorState
            icon="dns"
            title={t("extensions.mcp.loadFailed")}
            description={error}
            action={{ label: t("common.retry"), onClick: onRetry }}
          />
        </div>
      ) : servers.length === 0 ? (
        <div className="border border-dashed border-outline-variant/40 rounded-2xl bg-surface-container-low/30">
          <EmptyState
            compact
            icon="dns"
            title={t('extensions.mcp.addDialog.installed.empty.title')}
            description={t('extensions.mcp.addDialog.installed.empty.body')}
          />
        </div>
      ) : (
        <div className="border border-outline-variant/30 rounded-2xl overflow-hidden bg-surface-container-lowest/50">
          {servers.map((srv, i) => {
            const isBusy = busyId === `uninstall:${srv.name}`;
            const isRestarting = busyId === `restart:${srv.name}`;
            // W1-1 (R2-P0-1(B)): url-only OAuth/HTTP installs (the hub writes
            // `{"url":...}` with no command) are remote servers the desktop
            // pool cannot start yet — shown as Remote, never as Offline.
            const isRemote = !srv.command && !!srv.url;
            // Build a mono preview: command + args, or the remote endpoint.
            const preview = srv.command || srv.url || "";
            const rowStatusTitle = isRemote
              ? t("extensions.mcp.remoteHint")
              : srv.last_error ?? undefined;
            return (
              <div
                key={srv.name}
                className={cn(
                  "flex items-center gap-md px-md py-sm",
                  i !== servers.length - 1 && "border-b border-outline-variant/15",
                )}
              >
                <span className="material-symbols-outlined text-primary icon-md" aria-hidden="true">
                  {mcpServerIcon(srv.name)}
                </span>
                <div className="flex-1 min-w-0">
                  <div className="flex items-center gap-xs">
                    <div className="font-bold text-label-md text-on-surface truncate">
                      {srv.name}
                    </div>
                    {srv.connected ? (
                      <span className="text-label-xs px-xs py-[1px] rounded-full font-bold shrink-0 bg-primary-container text-on-primary-container">
                        {t("extensions.mcp.toolCount", {
                          count: srv.tool_count,
                        })}
                      </span>
                    ) : isRemote ? (
                      // W1-1: honest remote state — distinct from the Offline
                      // bad state, pointing at the CLI until desktop support
                      // for remote transports ships.
                      <span
                        className="text-label-xs px-xs py-[1px] rounded-full font-bold shrink-0 bg-tertiary-container text-on-tertiary-container"
                        title={t("extensions.mcp.remoteHint")}
                      >
                        {t("extensions.mcp.statusRemote")}
                      </span>
                    ) : (
                      <span
                        className="text-label-xs px-xs py-[1px] rounded-full font-bold shrink-0 bg-surface-container-highest text-on-surface-variant"
                        title={rowStatusTitle}
                      >
                        {t("extensions.mcp.offline")}
                      </span>
                    )}
                  </div>
                  {preview && (
                    <div className="text-label-xs text-on-surface-variant font-mono truncate">
                      {preview}
                    </div>
                  )}
                  {isRemote ? (
                    <div className="text-label-xs text-on-surface-variant truncate">
                      {t("extensions.mcp.remoteHint")}
                    </div>
                  ) : (
                    // W1-7 (R2-P1-6): a failed server shows its concrete
                    // error inline (full text on hover) — no more
                    // colour-only "Offline" dead ends.
                    srv.last_error && (
                      <div
                        className="text-label-xs text-error font-mono truncate"
                        title={srv.last_error}
                      >
                        {srv.last_error}
                      </div>
                    )
                  )}
                </div>
                {/* X3: per-server jump to the permissions page, pre-filtered
                    to this server's `mcp__<name>__*` rules. */}
                <Button
                  variant="ghost"
                  size="sm"
                  type="button"
                  aria-label={t("extensions.mcp.toolPermissionsAria", { name: srv.name })}
                  title={t("extensions.mcp.toolPermissionsAria", { name: srv.name })}
                  onClick={() => onOpenPermissions(srv.name)}
                  className="text-on-surface-variant hover:text-primary shrink-0"
                >
                  <span className="material-symbols-outlined icon-sm" aria-hidden="true">
                    key
                  </span>
                  {t("extensions.mcp.toolPermissions")}
                </Button>
                {/* G1 P0-1.4 — restart the server process (stop + start)
                    without leaving the page. W1-1: disabled for url-only
                    remote servers (no stdio process to restart) with a
                    tooltip explaining why. */}
                <Button
                  variant="ghost"
                  size="sm"
                  type="button"
                  aria-label={
                    isRemote
                      ? t("extensions.mcp.restartRemoteDisabled")
                      : t("extensions.mcp.restartAria", { name: srv.name })
                  }
                  title={
                    isRemote
                      ? t("extensions.mcp.restartRemoteDisabled")
                      : t("extensions.mcp.restartAria", { name: srv.name })
                  }
                  onClick={() => onRestart(srv.name)}
                  disabled={isBusy || isRestarting || isRemote}
                  className="text-on-surface-variant hover:text-primary shrink-0"
                >
                  <span className="material-symbols-outlined icon-sm" aria-hidden="true">
                    restart_alt
                  </span>
                  {isRestarting ? "…" : t("extensions.mcp.restart")}
                </Button>
                <Button
                  variant="ghost"
                  size="sm"
                  type="button"
                  onClick={() => onUninstall(srv.name)}
                  disabled={isBusy}
                  className="bg-error-container text-on-error-container hover:brightness-95"
                >
                  {isBusy ? "…" : t("extensions.mcp.remove")}
                </Button>
              </div>
            );
          })}
        </div>
      )}
    </section>
  );
}
