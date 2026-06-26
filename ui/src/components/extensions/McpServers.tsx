import { useEffect, useState } from "react";
import { useOutletContext } from "react-router-dom";
import { useIntl } from "react-intl";
import { toast } from "sonner";
import {
  listMcpServers,
  uninstallMcpServer,
} from "@/lib/tauri-api";
import { safeErrorMessage } from "@/lib/packageValidation";
import type { McpServerInfo } from "@/types";
import McpAddServerDialog from "./McpAddServerDialog";
import { ConfirmDialog } from "@/components/ui/confirm-dialog";
import LoadingState from "@/components/ui/loading-state";

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

  // The Extensions shell pipes a shared search box down via outlet context.
  // We no longer render the registry inline, so the value is only consulted
  // when the user opens the modal (the modal reads it as the initial query).
  const { search } = useOutletContext<{ search: string }>();

  const [installed, setInstalled] = useState<McpServerInfo[]>([]);
  const [installedLoading, setInstalledLoading] = useState(true);
  const [busyId, setBusyId] = useState<string | null>(null);
  const [dialogOpen, setDialogOpen] = useState(false);
  const [removeTarget, setRemoveTarget] = useState<string | null>(null);

  const refreshInstalled = () => {
    listMcpServers()
      .then((rows) => setInstalled(rows))
      .finally(() => setInstalledLoading(false));
  };

  useEffect(() => {
    refreshInstalled();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  async function handleUninstall(name: string) {
    setBusyId(`uninstall:${name}`);
    try {
      await uninstallMcpServer(name);
      toast.success(t("extensions.mcp.removed", { name }));
      refreshInstalled();
    } catch (err) {
      toast.error(
        intl.formatMessage(
          { id: "extensions.mcp.oneClick.installFailed" },
          { error: safeErrorMessage(err, "uninstall failed") },
        ),
      );
    } finally {
      setBusyId(null);
    }
  }

  function handleInstalled() {
    refreshInstalled();
  }

  return (
    <div className="p-lg max-w-6xl mx-auto space-y-xl">
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
        busyId={busyId}
        onUninstall={(name) => setRemoveTarget(name)}
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
        <button
          type="button"
          onClick={() => setDialogOpen(true)}
          className="inline-flex items-center gap-xs px-lg py-sm rounded-xl bg-primary text-on-primary text-label-md font-bold hover:bg-primary/90 cursor-pointer"
        >
          <span className="material-symbols-outlined text-[18px]">add</span>
          {t("extensions.mcp.addDialog.cta")}
        </button>
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
  busyId,
  onUninstall,
}: {
  servers: McpServerInfo[];
  loading: boolean;
  busyId: string | null;
  onUninstall: (name: string) => void;
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
      ) : servers.length === 0 ? (
        <div className="border border-dashed border-outline-variant/40 rounded-2xl p-lg text-center bg-surface-container-low/30">
          <span className="material-symbols-outlined text-[32px] text-on-surface-variant mb-xs inline-block">
            dns
          </span>
          <div className="font-bold text-label-md text-on-surface mb-xs">
            {t("extensions.mcp.addDialog.installed.empty.title")}
          </div>
          <p className="text-label-sm text-on-surface-variant max-w-md mx-auto">
            {t("extensions.mcp.addDialog.installed.empty.body")}
          </p>
        </div>
      ) : (
        <div className="border border-outline-variant/30 rounded-2xl overflow-hidden bg-surface-container-lowest/50">
          {servers.map((srv, i) => {
            const isBusy = busyId === `uninstall:${srv.name}`;
            // Build a mono preview: command + args (truncated)
            const preview = [srv.command].filter(Boolean).join(" ");
            return (
              <div
                key={srv.name}
                className={`flex items-center gap-md px-md py-sm ${
                  i === servers.length - 1
                    ? ""
                    : "border-b border-outline-variant/15"
                }`}
              >
                <span className="material-symbols-outlined text-primary text-[20px]" aria-hidden="true">
                  {mcpServerIcon(srv.name)}
                </span>
                <div className="flex-1 min-w-0">
                  <div className="flex items-center gap-xs">
                    <div className="font-bold text-label-md text-on-surface truncate">
                      {srv.name}
                    </div>
                    <span
                      className={`text-label-xs px-xs py-[1px] rounded-full font-bold shrink-0 ${
                        srv.connected
                          ? "bg-primary-container/60 text-on-primary-container"
                          : "bg-surface-container-highest text-on-surface-variant"
                      }`}
                    >
                      {srv.connected
                        ? t("extensions.mcp.toolCount", {
                            count: srv.tool_count,
                          })
                        : t("extensions.mcp.offline")}
                    </span>
                  </div>
                  {preview && (
                    <div className="text-label-xs text-on-surface-variant font-mono truncate">
                      {preview}
                    </div>
                  )}
                </div>
                <button
                  type="button"
                  onClick={() => onUninstall(srv.name)}
                  disabled={isBusy}
                  className="px-sm py-xs rounded-lg bg-error-container/40 text-on-error-container text-label-xs font-bold hover:bg-error-container/70 disabled:opacity-50"
                >
                  {isBusy ? "…" : t("extensions.mcp.remove")}
                </button>
              </div>
            );
          })}
        </div>
      )}
    </section>
  );
}
