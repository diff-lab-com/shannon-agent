// InstallDialog — modal that renders the right install form based on a
// CatalogEntry's source type.
//
// The dialog reads `entry.source` (CatalogSource discriminated union) and
// `entry.metadata` (untyped record from the Rust side) to pick one of four
// bodies:
//   1. featured_vendor + metadata.transport=oauth_remote → vendor info + a
//      placeholder "Connect" button (the OAuth installer is not wired yet).
//   2. git_hub_repo → install via installSkillFromRepo / installAgentFromRepo
//      with a busy spinner and success/error toast.
//   3. mcp_registry + metadata.package → preview the stdio command and install
//      via installMcpStdio.
//   4. native / fallback → tell the user to use the dedicated tab, with a
//      button that navigates via KIND_ROUTE.
//
// On a successful install the dialog dispatches `shannon:extension-installed`
// (same contract Plugins.tsx used before) so the Extensions shell can refresh.

import { useEffect, useState } from "react";
import { FormattedMessage, useIntl } from "react-intl";
import { useNavigate } from "react-router-dom";
import { toast } from "sonner";
import * as api from "@/lib/tauri-api";
import { isValidPackageName, safeErrorMessage } from "@/lib/packageValidation";
import type { AddonKind, CatalogEntry } from "@/types";
import { KIND_ROUTE } from "./Plugins";

export interface InstallDialogProps {
  entry: CatalogEntry | null;
  open: boolean;
  onClose: () => void;
  onInstalled: () => void;
}

/// Fields the catalog may stash in `CatalogEntry.metadata`. These are not part
/// of the Rust type yet — read defensively so a missing key never crashes UI.
interface OAuthMeta {
  transport?: string;
  endpoint?: string;
  scopes?: string[];
  vendor?: string;
}

interface PackageMeta {
  package?: { name?: string; type?: string };
}

type MaybeMeta = OAuthMeta & PackageMeta;

function readMeta(entry: CatalogEntry): MaybeMeta {
  return (entry.metadata ?? {}) as MaybeMeta;
}

/// Returns { command, args } for a registry package or null when the package
/// shape isn't one we recognise. Package name is strictly validated before
/// being placed in args so a malicious registry entry can't inject flags
/// (e.g. `--privileged`) or shell metacharacters.
function buildStdioSpec(
  pkgType: string | undefined,
  pkgName: string | undefined,
): { command: string; args: string[] } | null {
  if (!pkgType || !pkgName) return null;
  const kind = pkgType as "npm" | "pip" | "docker";
  if (!isValidPackageName(kind, pkgName)) return null;
  switch (kind) {
    case "npm":
      return { command: "npx", args: ["-y", pkgName] };
    case "pip":
      return { command: "pipx", args: ["run", pkgName] };
    case "docker":
      return { command: "docker", args: ["run", "-i", "--rm", pkgName] };
  }
}

export default function InstallDialog({
  entry,
  open,
  onClose,
  onInstalled,
}: InstallDialogProps) {
  const intl = useIntl();
  const navigate = useNavigate();
  const t = (id: string, values?: Record<string, string | number>) =>
    intl.formatMessage({ id }, values);

  const [installing, setInstalling] = useState(false);

  // Reset local state each time the dialog opens.
  useEffect(() => {
    if (open) setInstalling(false);
  }, [open]);

  // Escape closes the dialog.
  useEffect(() => {
    if (!open) return;
    const handler = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [open, onClose]);

  if (!open || !entry) return null;

  const meta = readMeta(entry);
  const route = KIND_ROUTE[entry.kind as AddonKind];

  const dispatchInstalled = () => {
    window.dispatchEvent(
      new CustomEvent("shannon:extension-installed", {
        detail: { kind: entry.kind, name: entry.name },
      }),
    );
    onInstalled();
  };

  const handleGitHubInstall = async () => {
    if (entry.source.type !== "git_hub_repo") return;
    const ref_ = entry.source.ref_ || "main";
    setInstalling(true);
    try {
      let result: api.InstallResult;
      if (entry.kind === "skill") {
        result = await api.installSkillFromRepo(entry.name, entry.source.repo, ref_);
      } else if (entry.kind === "agent") {
        result = await api.installAgentFromRepo(entry.name, entry.source.repo, ref_);
      } else {
        // Other GitHub kinds aren't backed by a dedicated installer yet.
        toast.error(t("extensions.plugins.installError", { error: entry.kind }));
        return;
      }
      toast.success(
        intl.formatMessage(
          { id: "extensions.plugins.installSuccess" },
          { name: result.name },
        ),
      );
      dispatchInstalled();
      onClose();
    } catch (e) {
      console.error("GitHub install error:", e);
      toast.error(
        intl.formatMessage(
          { id: "extensions.plugins.installError" },
          { error: safeErrorMessage(e, "install failed") },
        ),
      );
    } finally {
      setInstalling(false);
    }
  };

  const handleStdioInstall = async () => {
    const spec = buildStdioSpec(meta.package?.type, meta.package?.name);
    if (!spec) return;
    setInstalling(true);
    try {
      const result = await api.installMcpStdio({
        server_name: entry.name,
        command: spec.command,
        args: spec.args,
        env: [],
      });
      toast.success(
        intl.formatMessage(
          { id: "extensions.plugins.installSuccess" },
          { name: result.name },
        ),
      );
      dispatchInstalled();
      onClose();
    } catch (e) {
      console.error("MCP stdio install error:", e);
      toast.error(
        intl.formatMessage(
          { id: "extensions.plugins.installError" },
          { error: safeErrorMessage(e, "install failed") },
        ),
      );
    } finally {
      setInstalling(false);
    }
  };

  const handleOAuthConnect = () => {
    // Backend OAuth installer is not wired yet — be explicit so the user
    // isn't left wondering why nothing happened.
    toast.info(t("extensions.installDialog.oauthComingSoon"));
  };

  const handleOpenTab = () => {
    if (route) {
      navigate(route);
      onClose();
    }
  };

  // ---- Body selection ---------------------------------------------------

  const renderBody = () => {
    switch (entry.source.type) {
      case "featured_vendor": {
        if (meta.transport !== "oauth_remote") {
          // Featured vendor without OAuth metadata — fall through to the
          // "manual configuration" branch.
          break;
        }
        const scopes = meta.scopes ?? [];
        return (
          <div className="flex flex-col gap-md">
            <p className="text-label-sm text-on-surface-variant">
              {meta.vendor ?? entry.author ?? entry.name}
            </p>
            <label className="flex flex-col gap-xs">
              <span className="text-label-sm font-bold text-on-surface">
                <FormattedMessage id="extensions.installDialog.endpoint" />
              </span>
              <input
                readOnly
                value={meta.endpoint ?? ""}
                aria-label={t("extensions.installDialog.endpoint")}
                className="bg-surface-container-low border border-outline-variant/40 rounded-lg px-md py-sm font-body-md font-mono text-on-surface text-label-sm"
              />
            </label>
            <label className="flex flex-col gap-xs">
              <span className="text-label-sm font-bold text-on-surface">
                <FormattedMessage id="extensions.installDialog.scopes" />
              </span>
              <div className="flex flex-wrap gap-xs">
                {scopes.length === 0 ? (
                  <span className="text-label-sm text-on-surface-variant">—</span>
                ) : (
                  scopes.map((s) => (
                    <span
                      key={s}
                      className="px-xs py-[2px] rounded-full bg-surface-container-high text-label-xs text-on-surface-variant font-mono"
                    >
                      {s}
                    </span>
                  ))
                )}
              </div>
            </label>
            <button
              type="button"
              onClick={handleOAuthConnect}
              className="px-md py-sm rounded-lg bg-primary text-on-primary text-label-md font-bold hover:bg-primary/90 disabled:opacity-60 inline-flex items-center justify-center gap-xs cursor-pointer"
            >
              <span className="material-symbols-outlined text-[16px]">link</span>
              <FormattedMessage id="extensions.installDialog.connect" />
            </button>
          </div>
        );
      }
      case "git_hub_repo": {
        const ref_ = entry.source.ref_ || "main";
        return (
          <div className="flex flex-col gap-md">
            <div className="flex items-center gap-sm">
              <span className="material-symbols-outlined text-[18px] text-on-surface-variant">
                code
              </span>
              <span className="font-body-md font-mono text-on-surface">
                {entry.source.repo}
              </span>
            </div>
            <div className="flex items-center gap-sm">
              <span className="material-symbols-outlined text-[18px] text-on-surface-variant">
                commit
              </span>
              <span className="font-body-md font-mono text-on-surface">{ref_}</span>
            </div>
            <button
              type="button"
              onClick={handleGitHubInstall}
              disabled={installing}
              className="px-md py-sm rounded-lg bg-primary text-on-primary text-label-md font-bold hover:bg-primary/90 disabled:opacity-60 inline-flex items-center justify-center gap-xs cursor-pointer"
            >
              <span className="material-symbols-outlined text-[16px]">
                {installing ? "progress_activity" : "download"}
              </span>
              {installing ? (
                <FormattedMessage id="extensions.installDialog.installing" />
              ) : (
                <FormattedMessage id="extensions.installDialog.install" />
              )}
            </button>
          </div>
        );
      }
      case "mcp_registry": {
        const pkg = meta.package;
        const pkgType = pkg?.type;
        const spec = buildStdioSpec(pkgType, pkg?.name);
        return (
          <div className="flex flex-col gap-md">
            <label className="flex flex-col gap-xs">
              <span className="text-label-sm font-bold text-on-surface">
                <FormattedMessage id="extensions.installDialog.packageType" />
              </span>
              <span className="font-body-md text-on-surface">
                {pkgType ?? "—"}
              </span>
            </label>
            <label className="flex flex-col gap-xs">
              <span className="text-label-sm font-bold text-on-surface">
                <FormattedMessage id="extensions.installDialog.commandPreview" />
              </span>
              <code className="bg-surface-container-low border border-outline-variant/40 rounded-lg px-md py-sm font-body-md font-mono text-on-surface text-label-sm break-all">
                {spec ? [spec.command, ...spec.args].join(" ") : "—"}
              </code>
            </label>
            <button
              type="button"
              onClick={handleStdioInstall}
              disabled={installing || !spec}
              className="px-md py-sm rounded-lg bg-primary text-on-primary text-label-md font-bold hover:bg-primary/90 disabled:opacity-60 inline-flex items-center justify-center gap-xs cursor-pointer"
            >
              <span className="material-symbols-outlined text-[16px]">
                {installing ? "progress_activity" : "download"}
              </span>
              {installing ? (
                <FormattedMessage id="extensions.installDialog.installing" />
              ) : (
                <FormattedMessage id="extensions.installDialog.install" />
              )}
            </button>
          </div>
        );
      }
      case "custom":
      case "native":
      default:
        break;
    }

    // Fallback: manual configuration routed to the dedicated tab.
    return (
      <div className="flex flex-col gap-md">
        <p className="text-label-sm text-on-surface-variant">
          <FormattedMessage id="extensions.installDialog.manualHint" />
        </p>
        {route ? (
          <button
            type="button"
            onClick={handleOpenTab}
            className="px-md py-sm rounded-lg bg-primary text-on-primary text-label-md font-bold hover:bg-primary/90 inline-flex items-center justify-center gap-xs cursor-pointer"
          >
            <span className="material-symbols-outlined text-[16px]">tab</span>
            <FormattedMessage id="extensions.installDialog.openTab" />
          </button>
        ) : null}
      </div>
    );
  };

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/40 p-md"
      role="dialog"
      aria-modal="true"
      aria-labelledby="install-dialog-title"
      onClick={onClose}
    >
      <div
        onClick={(e) => e.stopPropagation()}
        className="bg-surface-container-lowest rounded-2xl shadow-2xl border border-outline-variant/40 w-full max-w-lg max-h-[90vh] overflow-y-auto p-lg flex flex-col gap-md"
      >
        <div className="flex items-center justify-between">
          <h2
            id="install-dialog-title"
            className="font-headline-md text-[18px] font-bold text-on-surface flex items-center gap-sm"
          >
            <span className="material-symbols-outlined text-[20px] text-primary">
              download
            </span>
            <FormattedMessage
              id="extensions.installDialog.title"
              values={{ name: entry.name }}
            />
          </h2>
          <button
            type="button"
            onClick={onClose}
            disabled={installing}
            aria-label={t("extensions.installDialog.closeAria")}
            className="text-on-surface-variant hover:bg-surface-container-high rounded-full p-xs cursor-pointer focus:outline-none focus-visible:ring-2 focus-visible:ring-primary/30 disabled:opacity-40"
          >
            <span className="material-symbols-outlined text-[18px]">close</span>
          </button>
        </div>

        {entry.description ? (
          <p className="text-label-sm text-on-surface-variant">
            {entry.description}
          </p>
        ) : null}

        {renderBody()}
      </div>
    </div>
  );
}
