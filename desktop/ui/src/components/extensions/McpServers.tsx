import { useEffect, useState } from "react";
import EmptyState from '@/components/ui/empty-state'
import ErrorState from '@/components/ui/error-state'
import { useNavigate, useOutletContext } from "react-router-dom";
import { useIntl } from "react-intl";
import { toast } from "sonner";
import {
  listMcpServers,
  reauthenticateMcpServer,
  restartMcpServer,
  setMcpServerEnabled,
  uninstallMcpServer,
} from "@/lib/tauri-api";
import { safeErrorMessage } from "@/lib/packageValidation";
import type { McpServerInfo } from "@/types";
import McpAddServerDialog from "./McpAddServerDialog";
import { ConfirmDialog } from "@/components/ui/confirm-dialog";
import LoadingState from "@/components/ui/loading-state";
import { Switch } from "@/components/ui/switch";
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

  // F5 (A8): the page's credential-storage status line — where the
  // servers' OAuth tokens live. Derived from the rows the backend already
  // reports (any credential-bearing row answers for the page; the backend
  // marks a degraded row `plaintext_file` even while other rows are in the
  // keyring, so the honest mode wins).
  const storageMode = installed.some((s) => s.credential_storage === 'plaintext_file')
    ? 'plaintext_file'
    : (installed.find((s) => s.credential_storage)?.credential_storage ?? null);

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

  // W3-B (A2): the NeedsAuth recovery action — replay the OAuth loopback
  // flow for this entry and reconnect, without a restart.
  async function handleReauthenticate(name: string) {
    setBusyId(`reauth:${name}`);
    try {
      await reauthenticateMcpServer(name);
      toast.success(t("extensions.mcp.reauthSuccess", { name }));
    } catch (err) {
      toast.error(
        intl.formatMessage(
          { id: "extensions.mcp.reauthFailed" },
          { error: safeErrorMessage(err, "re-authentication failed") },
        ),
      );
    } finally {
      setBusyId(null);
      refreshInstalled();
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

  // W2-A (R4/A1) — inline enable/disable: persists to the unified
  // settings.json store and reconciles the pool (stop on disable, start on
  // enable; auth-gated remote rows stay in the honest state).
  async function handleToggle(name: string, enabled: boolean) {
    setBusyId(`toggle:${name}`);
    try {
      await setMcpServerEnabled(name, enabled);
    } catch (err) {
      toast.error(
        intl.formatMessage(
          { id: "extensions.mcp.toggleFailed" },
          { name, error: safeErrorMessage(err, "update failed") },
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
        {storageMode && (
          <p
            className="text-body-sm text-on-surface-variant mt-xs flex items-center gap-xs"
            data-testid="mcp-credential-storage"
          >
            <span className="material-symbols-outlined icon-sm" aria-hidden="true">
              key
            </span>
            {t("extensions.credentialStorage.label")}:{" "}
            {storageMode === "keyring"
              ? t("extensions.credentialStorage.keyring")
              : t("extensions.credentialStorage.file")}
          </p>
        )}
      </header>

      <InstalledSection
        servers={installed}
        loading={installedLoading}
        error={installedError}
        onRetry={refreshInstalled}
        busyId={busyId}
        onUninstall={(name) => setRemoveTarget(name)}
        onRestart={handleRestart}
        onToggle={handleToggle}
        onReauthenticate={handleReauthenticate}
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

/** W3-B (A2): the failure presentation of a remote row. The three failure
 *  classes (assessment draft W3-3) each get their own semantics — they
 *  never collapse into one generic Offline badge. */
type RemoteFailureState = 'needs_auth' | 'unreachable' | 'server_error'

function remoteFailureState(srv: McpServerInfo): RemoteFailureState | null {
  if (srv.enabled && srv.connected) return null
  if (!srv.enabled) return null
  switch (srv.failure_kind) {
    case 'needs_auth':
      return 'needs_auth'
    case 'unreachable':
      return 'unreachable'
    case 'server_error':
      return 'server_error'
    default:
      // Not connected, no classified error yet (fresh seed or legacy row):
      // an OAuth entry that is up-but-unreachable-by-credential renders as
      // NeedsAuth; anything else stays neutral (null → stdio Offline path).
      return srv.has_auth_headers && srv.url ? 'needs_auth' : null
  }
}

function InstalledSection({
  servers,
  loading,
  error,
  onRetry,
  busyId,
  onUninstall,
  onRestart,
  onToggle,
  onReauthenticate,
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
  onToggle: (name: string, enabled: boolean) => void;
  onReauthenticate: (name: string) => void;
  onOpenPermissions: (name: string) => void;
}) {
  const intl = useIntl();
  const t = (id: string, values?: Record<string, string | number>) =>
    intl.formatMessage({ id }, values);
  // W3-B: per-row expanded detail (ServerError's "view details" action).
  const [detailsFor, setDetailsFor] = useState<string | null>(null);

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
            const isToggling = busyId === `toggle:${srv.name}`;
            const isRestarting = busyId === `restart:${srv.name}`;
            const isReauthing = busyId === `reauth:${srv.name}`;
            const failure = remoteFailureState(srv);
            // Build a mono preview: command + args, or the remote endpoint.
            const preview = srv.command || srv.url || "";
            const statusBadge = (() => {
              if (!srv.enabled) {
                return (
                  <span
                    className="text-label-xs px-xs py-[1px] rounded-full font-bold shrink-0 bg-surface-container-highest text-on-surface-variant"
                    title={t("extensions.mcp.toggleAria", { name: srv.name })}
                  >
                    {t("extensions.mcp.statusDisabled")}
                  </span>
                );
              }
              if (srv.connected) {
                return (
                  <span
                    className="text-label-xs px-xs py-[1px] rounded-full font-bold shrink-0 bg-primary-container text-on-primary-container"
                    title={srv.last_error ?? undefined}
                  >
                    {t("extensions.mcp.statusOnline")}
                  </span>
                );
              }
              switch (failure) {
                case 'needs_auth':
                  // amber: login expired — distinct from Offline, points at
                  // re-authentication.
                  return (
                    <span
                      className="text-label-xs px-xs py-[1px] rounded-full font-bold shrink-0 bg-tertiary-container text-on-tertiary-container"
                      title={t("extensions.mcp.needsAuthHint")}
                    >
                      {t("extensions.mcp.statusNeedsAuth")}
                    </span>
                  );
                case 'unreachable':
                  // amber: network-level failure — retryable.
                  return (
                    <span
                      className="text-label-xs px-xs py-[1px] rounded-full font-bold shrink-0 bg-tertiary-container text-on-tertiary-container"
                      title={srv.last_error ?? t("extensions.mcp.unreachableHint")}
                    >
                      {t("extensions.mcp.statusUnreachable")}
                    </span>
                  );
                case 'server_error':
                  // red: the server answered with an error — retry later.
                  return (
                    <span
                      className="text-label-xs px-xs py-[1px] rounded-full font-bold shrink-0 bg-error-container text-on-error-container"
                      title={srv.last_error ?? t("extensions.mcp.serverErrorHint")}
                    >
                      {t("extensions.mcp.statusServerError")}
                    </span>
                  );
                default:
                  return (
                    <span
                      className="text-label-xs px-xs py-[1px] rounded-full font-bold shrink-0 bg-surface-container-highest text-on-surface-variant"
                      title={srv.last_error ?? undefined}
                    >
                      {t("extensions.mcp.offline")}
                    </span>
                  );
              }
            })();
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
                    {/* W2-A: connection status and tool count are two
                        separate elements — the status pill says *whether*
                        the server is up, the chip says *what* it offers. */}
                    {statusBadge}
                    {/* Tool count chip — its own element next to the
                        status pill (was fused into it before W2-A). */}
                    {srv.enabled && srv.connected && (
                      <span className="text-label-xs px-xs py-[1px] rounded-full font-bold shrink-0 bg-secondary-container text-on-secondary-container">
                        {t("extensions.mcp.toolCount", {
                          count: srv.tool_count,
                        })}
                      </span>
                    )}
                  </div>
                  {preview && (
                    <div className="text-label-xs text-on-surface-variant font-mono truncate">
                      {preview}
                    </div>
                  )}
                  {/* W3-B failure lines — each class gets its own copy and
                      actions; NeedsAuth additionally keeps the url/endpoint
                      context. Never a colour-only dead end. */}
                  {srv.enabled && failure === 'needs_auth' && (
                    <div className="text-label-xs text-on-surface-variant truncate">
                      {t("extensions.mcp.needsAuthHint")}
                    </div>
                  )}
                  {srv.enabled && failure === 'unreachable' && srv.last_error && (
                    <div
                      className="text-label-xs text-error font-mono truncate"
                      title={srv.last_error}
                    >
                      {srv.last_error}
                    </div>
                  )}
                  {srv.enabled && failure === 'server_error' && srv.last_error && (
                    <div className="flex items-center gap-xs min-w-0">
                      <div
                        className="text-label-xs text-error font-mono truncate"
                        title={srv.last_error}
                      >
                        {srv.last_error}
                      </div>
                      <Button
                        variant="ghost"
                        size="sm"
                        type="button"
                        className="text-on-surface-variant hover:text-primary shrink-0 !px-1 !py-0 h-auto"
                        onClick={() =>
                          setDetailsFor(detailsFor === srv.name ? null : srv.name)
                        }
                      >
                        {detailsFor === srv.name
                          ? t("extensions.mcp.hideDetails")
                          : t("extensions.mcp.viewDetails")}
                      </Button>
                    </div>
                  )}
                  {srv.enabled &&
                    failure === 'server_error' &&
                    detailsFor === srv.name &&
                    srv.last_error && (
                      <pre className="text-label-xs text-error font-mono whitespace-pre-wrap break-all mt-xs p-xs rounded-lg bg-error-container/40 border border-outline-variant/30">
                        {srv.last_error}
                      </pre>
                    )}
                  {srv.enabled &&
                    !failure &&
                    srv.last_error && (
                      // W1-7 (R2-P1-6): a failed stdio server shows its
                      // concrete error inline (full text on hover) — no
                      // colour-only "Offline" dead ends.
                      <div
                        className="text-label-xs text-error font-mono truncate"
                        title={srv.last_error}
                      >
                        {srv.last_error}
                      </div>
                    )}
                </div>
                {/* W2-A: inline enable/disable — the backend persists the
                    flag and reconciles the pool (stop on disable, start on
                    enable). */}
                <Switch
                  size="sm"
                  checked={srv.enabled}
                  disabled={isToggling}
                  onCheckedChange={(next) => onToggle(srv.name, next === true)}
                  aria-label={t("extensions.mcp.toggleAria", { name: srv.name })}
                  data-testid={`mcp-toggle-${srv.name}`}
                />
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
                {/* W3-B (A2): the failure-classes' recovery actions.
                    NeedsAuth → "Re-authenticate" (replays the OAuth loopback
                    flow); restart is disabled and points there, so a dead
                    credential can never fake success. Unreachable /
                    ServerError → "Retry" (a real pool reconnect). */}
                {failure === 'needs_auth' ? (
                  <Button
                    variant="outline"
                    size="sm"
                    type="button"
                    aria-label={t("extensions.mcp.reauthAria", { name: srv.name })}
                    title={t("extensions.mcp.reauthAria", { name: srv.name })}
                    onClick={() => onReauthenticate(srv.name)}
                    disabled={isBusy || isRestarting || isToggling || isReauthing}
                    className="shrink-0"
                    data-testid={`mcp-reauth-${srv.name}`}
                  >
                    <span className="material-symbols-outlined icon-sm" aria-hidden="true">
                      lock_open
                    </span>
                    {isReauthing ? "…" : t("extensions.mcp.reauth")}
                  </Button>
                ) : null}
                <Button
                  variant="ghost"
                  size="sm"
                  type="button"
                  aria-label={
                    failure === 'needs_auth'
                      ? t("extensions.mcp.restartNeedsAuthDisabled")
                      : t("extensions.mcp.restartAria", { name: srv.name })
                  }
                  title={
                    failure === 'needs_auth'
                      ? t("extensions.mcp.restartNeedsAuthDisabled")
                      : t("extensions.mcp.restartAria", { name: srv.name })
                  }
                  onClick={() => onRestart(srv.name)}
                  disabled={
                    isBusy || isRestarting || isToggling || isReauthing || failure === 'needs_auth'
                  }
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
