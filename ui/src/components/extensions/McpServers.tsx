import { useEffect, useRef, useState } from "react";
import { useOutletContext } from "react-router-dom";
import { useIntl } from "react-intl";
import {
  listMcpRegistryServers,
  installMcpStdio,
  installMcpMcpb,
  uninstallMcpServer,
  listMcpServers,
  type RegistryServer,
  type InstallResult,
  type StdioMcpSpecPayload,
} from "@/lib/tauri-api";
import type { McpServerInfo } from "@/types";

/**
 * Package metadata shipped by the MCP registry for one-click installs.
 * Mirrors `extensions::RegistryPackage` on the Rust side. Declared locally
 * because the shared `RegistryServer` interface in tauri-api.ts predates the
 * `package` field; the runtime payload still includes it.
 */
interface RegistryPackage {
  kind: string; // "npm" | "pip" | "docker" | …
  name?: string;
  registry_url?: string;
  version?: string;
}

/**
 * `RegistryServer` augmented with the optional `package` field that the
 * backend serializes but the shared TS interface does not yet declare.
 */
type RegistryServerWithPackage = RegistryServer & { package?: RegistryPackage | null };

const STDIO_MANUAL_FORM_ID = "mcp-stdio-manual-form";

/**
 * Build a stdio spec from registry package metadata. Returns `null` when the
 * package kind is unknown or required fields (e.g. npm/pip/docker name) are
 * missing — callers should fall back to the manual form.
 */
function buildSpecFromPackage(
  serverName: string,
  pkg: RegistryPackage | null | undefined,
): StdioMcpSpecPayload | null {
  if (!pkg) return null;
  const name = pkg.name?.trim();
  const versionSuffix = pkg.version?.trim() ? `@${pkg.version.trim()}` : "";
  switch (pkg.kind) {
    case "npm":
      if (!name) return null;
      return {
        server_name: serverName,
        command: "npx",
        args: ["-y", versionSuffix ? `${name}${versionSuffix}` : name],
        env: [],
      };
    case "pip":
      if (!name) return null;
      return {
        server_name: serverName,
        command: "uvx",
        args: [name],
        env: [],
      };
    case "docker":
      if (!name) return null;
      return {
        server_name: serverName,
        command: "docker",
        args: ["run", "-i", "--rm", name],
        env: [],
      };
    default:
      return null;
  }
}

/** Human-readable label for the package manager backing a one-click install. */
function packageManagerLabel(pkg: RegistryPackage | null | undefined): string | null {
  if (!pkg) return null;
  switch (pkg.kind) {
    case "npm":
      return "npx";
    case "pip":
      return "uvx";
    case "docker":
      return "docker";
    default:
      return null;
  }
}

/**
 * P2 MCP Servers tab — registry browser + .mcpb upload + manual stdio form.
 *
 * Three flows:
 * 1. Browse the federated MCP Registry (24h cache). Install button triggers
 *    the resolver → dispatches to OAuthRemote / Mcpb / Stdio.
 * 2. Upload `.mcpb` archive from disk → install_mcp_mcpb.
 * 3. Fill command/args/env by hand → install_mcp_stdio (Tier-3 escape hatch).
 *
 * Installed servers are listed at the bottom with uninstall buttons.
 */
export default function McpServers() {
  const intl = useIntl();
  const t = (id: string) => intl.formatMessage({ id });
  const { search } = useOutletContext<{ search: string }>();

  const [registry, setRegistry] = useState<RegistryServer[]>([]);
  const [registryLoading, setRegistryLoading] = useState(true);
  const [registryError, setRegistryError] = useState<string | null>(null);

  const [installed, setInstalled] = useState<McpServerInfo[]>([]);
  const [installedLoading, setInstalledLoading] = useState(true);

  const [busyId, setBusyId] = useState<string | null>(null);
  const [feedback, setFeedback] = useState<{ id: string; msg: string; ok: boolean } | null>(null);

  useEffect(() => {
    let cancelled = false;
    setRegistryLoading(true);
    listMcpRegistryServers()
      .then((rows) => {
        if (!cancelled) {
          setRegistry(rows);
          setRegistryError(null);
        }
      })
      .catch((err) => {
        if (!cancelled) setRegistryError(String(err));
      })
      .finally(() => {
        if (!cancelled) setRegistryLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, []);

  const refreshInstalled = () => {
    listMcpServers()
      .then((rows) => setInstalled(rows))
      .finally(() => setInstalledLoading(false));
  };

  useEffect(() => {
    refreshInstalled();
  }, []);

  async function handleInstall(server: RegistryServer) {
    const serverWithPkg = server as RegistryServerWithPackage;
    const spec = buildSpecFromPackage(server.name, serverWithPkg.package);
    if (!spec) {
      setFeedback({
        id: server.id,
        msg: intl.formatMessage({ id: "extensions.mcp.oneClick.noPackageMetadata" }),
        ok: false,
      });
      document
        .getElementById(STDIO_MANUAL_FORM_ID)
        ?.scrollIntoView({ behavior: "smooth", block: "start" });
      return;
    }
    setBusyId(server.id);
    setFeedback(null);
    try {
      await installMcpStdio(spec);
      setFeedback({
        id: server.id,
        msg: intl.formatMessage(
          { id: "extensions.mcp.oneClick.installSuccess" },
          { name: server.name },
        ),
        ok: true,
      });
      refreshInstalled();
    } catch (err) {
      setFeedback({
        id: server.id,
        msg: intl.formatMessage(
          { id: "extensions.mcp.oneClick.installFailed" },
          { error: String(err) },
        ),
        ok: false,
      });
    } finally {
      setBusyId(null);
    }
  }

  async function handleUninstall(name: string) {
    setBusyId(`uninstall:${name}`);
    setFeedback(null);
    try {
      await uninstallMcpServer(name);
      setFeedback({ id: `uninstall:${name}`, msg: intl.formatMessage({ id: 'extensions.mcp.removed' }, { name }), ok: true });
      refreshInstalled();
    } catch (err) {
      setFeedback({ id: `uninstall:${name}`, msg: String(err), ok: false });
    } finally {
      setBusyId(null);
    }
  }

  const filteredRegistry = search
    ? registry.filter(
        (s) =>
          s.name.toLowerCase().includes(search.toLowerCase()) ||
          (s.description ?? "").toLowerCase().includes(search.toLowerCase())
      )
    : registry;

  return (
    <div className="p-lg max-w-5xl mx-auto space-y-xl">
      <header>
        <h2 className="text-headline-md font-bold text-on-surface mb-xs">{t('extensions.mcp.title')}</h2>
        <p className="text-body-md text-on-surface-variant">
          {t('extensions.mcp.subtitle')}
        </p>
      </header>

      <RegistrySection
        servers={filteredRegistry}
        loading={registryLoading}
        error={registryError}
        busyId={busyId}
        feedback={feedback}
        onInstall={handleInstall}
        installedNames={new Set(installed.map((s) => s.name))}
      />

      <McpbUploadSection
        onResult={(r) =>
          setFeedback({ id: `mcpb:${r.name}`, msg: `Installed ${r.name}`, ok: true })
        }
        onError={(msg) =>
          setFeedback({ id: "mcpb:err", msg, ok: false })
        }
        refreshInstalled={refreshInstalled}
      />

      <StdioManualForm
        onResult={(r) =>
          setFeedback({ id: `stdio:${r.name}`, msg: `Installed ${r.name}`, ok: true })
        }
        onError={(msg) =>
          setFeedback({ id: "stdio:err", msg, ok: false })
        }
        refreshInstalled={refreshInstalled}
      />

      <InstalledSection
        servers={installed}
        loading={installedLoading}
        busyId={busyId}
        onUninstall={handleUninstall}
      />

      {feedback && (
        <div
          className={`text-label-sm ${feedback.ok ? "text-primary" : "text-error"}`}
        >
          {feedback.msg}
        </div>
      )}
    </div>
  );
}

// ---------------------------------------------------------------------------
// Registry browser
// ---------------------------------------------------------------------------

function RegistrySection({
  servers,
  loading,
  error,
  busyId,
  feedback,
  onInstall,
  installedNames,
}: {
  servers: RegistryServer[];
  loading: boolean;
  error: string | null;
  busyId: string | null;
  feedback: { id: string; msg: string; ok: boolean } | null;
  onInstall: (server: RegistryServer) => void;
  installedNames: Set<string>;
}) {
  const intl = useIntl();
  const t = (id: string) => intl.formatMessage({ id });
  return (
    <section>
      <h3 className="text-label-lg font-bold text-on-surface-variant uppercase tracking-wide mb-sm">
        {t('extensions.mcp.registry')} · {servers.length}
      </h3>

      {loading && (
        <div className="text-center py-lg text-on-surface-variant">
          <span className="material-symbols-outlined animate-spin align-middle mr-xs">progress_activity</span>
          {t('extensions.mcp.fetching')}
        </div>
      )}

      {error && (
        <div className="border border-error/30 rounded-xl p-md bg-error-container/10 text-label-sm text-error">
          {t('extensions.mcp.registryError')} <span className="font-mono">{error}</span>
        </div>
      )}

      {!loading && !error && servers.length === 0 && (
        <div className="text-center py-lg text-on-surface-variant text-label-md">
          {t('extensions.mcp.noServers')}
        </div>
      )}

      {!loading && !error && servers.length > 0 && (
        <div className="grid grid-cols-1 md:grid-cols-2 gap-md">
          {servers.map((server) => {
            const isBusy = busyId === server.id;
            const isInstalled = installedNames.has(server.name);
            const serverFeedback = feedback?.id === server.id ? feedback : null;
            const pkgManager = packageManagerLabel(
              (server as RegistryServerWithPackage).package,
            );
            return (
              <div
                key={server.id}
                className="border border-outline-variant/30 rounded-2xl p-md bg-surface-container-low/40 flex flex-col"
              >
                <div className="flex items-start justify-between mb-xs">
                  <div className="min-w-0">
                    <div className="flex items-center gap-xs">
                      <h4 className="font-bold text-label-md text-on-surface truncate">{server.name}</h4>
                      {server.verified && (
                        <span className="text-label-xs px-xs py-[1px] rounded-full bg-primary-container/60 text-on-primary-container font-bold">
                          Verified
                        </span>
                      )}
                      {isInstalled && (
                        <span className="text-label-xs px-xs py-[1px] rounded-full bg-secondary-container/60 text-on-secondary-container font-bold">
                          Installed
                        </span>
                      )}
                    </div>
                    {server.version && (
                      <span className="text-label-xs text-on-surface-variant font-mono">
                        {server.version}
                      </span>
                    )}
                  </div>
                  {server.stars != null && (
                    <div className="flex items-center gap-[2px] text-label-xs text-on-surface-variant shrink-0">
                      <span className="material-symbols-outlined text-[14px]">star</span>
                      {server.stars}
                    </div>
                  )}
                </div>
                {server.description && (
                  <p className="text-label-sm text-on-surface-variant flex-1 mb-sm line-clamp-2">
                    {server.description}
                  </p>
                )}
                {serverFeedback && (
                  <div
                    className={`text-label-xs mb-xs ${serverFeedback.ok ? "text-primary" : "text-error"}`}
                  >
                    {serverFeedback.msg}
                  </div>
                )}
                <div className="flex gap-xs">
                  {server.repository && (
                    <a
                      href={server.repository}
                      target="_blank"
                      rel="noreferrer"
                      className="px-sm py-xs rounded-lg bg-surface-container-high text-on-surface text-label-xs font-bold hover:bg-surface-container-highest"
                    >
                      {t('extensions.mcp.repo')}
                    </a>
                  )}
                  <button
                    type="button"
                    onClick={() => onInstall(server)}
                    disabled={isBusy || isInstalled}
                    className="px-sm py-xs rounded-lg bg-primary text-on-primary text-label-xs font-bold hover:bg-primary/90 disabled:opacity-50 disabled:cursor-not-allowed"
                  >
                    {isBusy ? "…" : isInstalled ? t('extensions.mcp.installed') : t('extensions.mcp.install')}
                  </button>
                  {pkgManager && !isInstalled && (
                    <span
                      className="inline-flex items-center text-on-surface-variant"
                      title={intl.formatMessage(
                        { id: "extensions.mcp.oneClick.autoInstallHint" },
                        { packageManager: pkgManager },
                      )}
                    >
                      <span
                        className="material-symbols-outlined text-[16px] cursor-help"
                        aria-label={intl.formatMessage(
                          { id: "extensions.mcp.oneClick.autoInstallHint" },
                          { packageManager: pkgManager },
                        )}
                      >
                        help_outline
                      </span>
                    </span>
                  )}
                </div>
              </div>
            );
          })}
        </div>
      )}
    </section>
  );
}

// ---------------------------------------------------------------------------
// .mcpb upload
// ---------------------------------------------------------------------------

function McpbUploadSection({
  onResult,
  onError,
  refreshInstalled,
}: {
  onResult: (r: InstallResult) => void;
  onError: (msg: string) => void;
  refreshInstalled: () => void;
}) {
  const intl = useIntl();
  const t = (id: string) => intl.formatMessage({ id });
  const inputRef = useRef<HTMLInputElement>(null);
  const [name, setName] = useState("");
  const [busy, setBusy] = useState(false);

  async function handleFile(file: File) {
    if (!name.trim()) {
      onError(t('extensions.mcp.needName'));
      return;
    }
    setBusy(true);
    try {
      const buf = new Uint8Array(await file.arrayBuffer());
      const r = await installMcpMcpb(name.trim(), Array.from(buf));
      onResult(r);
      refreshInstalled();
      setName("");
      if (inputRef.current) inputRef.current.value = "";
    } catch (err) {
      onError(String(err));
    } finally {
      setBusy(false);
    }
  }

  return (
    <section className="border border-outline-variant/30 rounded-2xl p-md bg-surface-container-low/30">
      <h3 className="text-label-lg font-bold text-on-surface mb-xs">{t('extensions.mcp.uploadTitle')}</h3>
      <p className="text-label-sm text-on-surface-variant mb-sm">
        {t('extensions.mcp.uploadDesc')}
      </p>
      <div className="flex flex-wrap gap-xs items-end">
        <label className="flex-1 min-w-[200px]">
          <span className="block text-label-xs text-on-surface-variant mb-[2px]">{t('extensions.mcp.serverName')}</span>
          <input
            type="text"
            value={name}
            onChange={(e) => setName(e.target.value)}
            placeholder="e.g. my-vendor-mcp"
            className="w-full px-sm py-xs rounded border border-outline-variant text-label-sm bg-surface"
            disabled={busy}
          />
        </label>
        <input
          ref={inputRef}
          type="file"
          accept=".mcpb,.zip,application/zip"
          onChange={(e) => {
            const f = e.target.files?.[0];
            if (f) handleFile(f);
          }}
          disabled={busy}
          className="block text-label-sm text-on-surface-variant file:mr-xs file:px-sm file:py-xs file:rounded file:border-0 file:bg-primary file:text-on-primary file:font-bold file:cursor-pointer"
        />
      </div>
      {busy && (
        <p className="text-label-xs text-on-surface-variant mt-xs">
          <span className="material-symbols-outlined animate-spin align-middle mr-xs text-[14px]">progress_activity</span>
          {t('extensions.mcp.extracting')}
        </p>
      )}
    </section>
  );
}

// ---------------------------------------------------------------------------
// Manual stdio form
// ---------------------------------------------------------------------------

function StdioManualForm({
  onResult,
  onError,
  refreshInstalled,
}: {
  onResult: (r: InstallResult) => void;
  onError: (msg: string) => void;
  refreshInstalled: () => void;
}) {
  const intl = useIntl();
  const t = (id: string) => intl.formatMessage({ id });
  const [name, setName] = useState("");
  const [command, setCommand] = useState("");
  const [argsText, setArgsText] = useState("");
  const [envText, setEnvText] = useState("");
  const [busy, setBusy] = useState(false);

  function parseArgs(text: string): string[] {
    // Naive whitespace split, with quote support.
    const out: string[] = [];
    const re = /"([^"]*)"|'([^']*)'|(\S+)/g;
    let m: RegExpExecArray | null;
    while ((m = re.exec(text)) !== null) {
      out.push(m[1] ?? m[2] ?? m[3] ?? "");
    }
    return out;
  }

  function parseEnv(text: string): [string, string][] {
    return text
      .split(/\n/)
      .map((line) => line.trim())
      .filter(Boolean)
      .map((line) => {
        const eq = line.indexOf("=");
        if (eq < 0) return [line, ""] as [string, string];
        return [line.slice(0, eq).trim(), line.slice(eq + 1).trim()] as [string, string];
      });
  }

  async function handleSubmit() {
    if (!name.trim() || !command.trim()) {
      onError(t('extensions.mcp.needNameAndCommand'));
      return;
    }
    setBusy(true);
    try {
      const spec: StdioMcpSpecPayload = {
        server_name: name.trim(),
        command: command.trim(),
        args: parseArgs(argsText),
        env: parseEnv(envText),
      };
      const r = await installMcpStdio(spec);
      onResult(r);
      refreshInstalled();
      setName("");
      setCommand("");
      setArgsText("");
      setEnvText("");
    } catch (err) {
      onError(String(err));
    } finally {
      setBusy(false);
    }
  }

  return (
    <section
      id={STDIO_MANUAL_FORM_ID}
      className="border border-outline-variant/30 rounded-2xl p-md bg-surface-container-low/30 scroll-mt-md"
    >
      <h3 className="text-label-lg font-bold text-on-surface mb-xs">{t('extensions.mcp.addStdioTitle')}</h3>
      <p className="text-label-sm text-on-surface-variant mb-sm">
        {t('extensions.mcp.manualDesc')}
      </p>
      <div className="space-y-sm">
        <label className="block">
          <span className="block text-label-xs text-on-surface-variant mb-[2px]">{t('extensions.mcp.serverNameRequired')}</span>
          <input
            type="text"
            value={name}
            onChange={(e) => setName(e.target.value)}
            placeholder="filesystem"
            className="w-full px-sm py-xs rounded border border-outline-variant text-label-sm bg-surface"
            disabled={busy}
          />
        </label>
        <label className="block">
          <span className="block text-label-xs text-on-surface-variant mb-[2px]">{t('extensions.mcp.commandRequired')}</span>
          <input
            type="text"
            value={command}
            onChange={(e) => setCommand(e.target.value)}
            placeholder="npx"
            className="w-full px-sm py-xs rounded border border-outline-variant text-label-sm bg-surface font-mono"
            disabled={busy}
          />
        </label>
        <label className="block">
          <span className="block text-label-xs text-on-surface-variant mb-[2px]">{t('extensions.mcp.argsLabel')}</span>
          <input
            type="text"
            value={argsText}
            onChange={(e) => setArgsText(e.target.value)}
            placeholder='-y @modelcontextprotocol/server-filesystem /tmp'
            className="w-full px-sm py-xs rounded border border-outline-variant text-label-sm bg-surface font-mono"
            disabled={busy}
          />
        </label>
        <label className="block">
          <span className="block text-label-xs text-on-surface-variant mb-[2px]">{t('extensions.mcp.envLabel')}</span>
          <textarea
            value={envText}
            onChange={(e) => setEnvText(e.target.value)}
            rows={3}
            placeholder={"ROOT=/tmp\nLOG_LEVEL=info"}
            className="w-full px-sm py-xs rounded border border-outline-variant text-label-sm bg-surface font-mono"
            disabled={busy}
          />
        </label>
        <button
          type="button"
          onClick={handleSubmit}
          disabled={busy}
          className="inline-flex items-center gap-xs px-md py-sm rounded-lg bg-primary text-on-primary text-label-sm font-bold hover:bg-primary/90 disabled:opacity-50"
        >
          {busy ? (
            <>
              <span className="material-symbols-outlined text-[16px] animate-spin">progress_activity</span>
              {t('extensions.mcp.installing')}
            </>
          ) : (
            <>
              <span className="material-symbols-outlined text-[16px]">add</span>
              {t('extensions.mcp.install')}
            </>
          )}
        </button>
      </div>
    </section>
  );
}

// ---------------------------------------------------------------------------
// Installed servers (with uninstall)
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
  const t = (id: string) => intl.formatMessage({ id });
  return (
    <section>
      <h3 className="text-label-lg font-bold text-on-surface-variant uppercase tracking-wide mb-sm">
        {t('extensions.mcp.installedSection')} · {servers.length}
      </h3>
      {loading ? (
        <div className="text-center py-md text-on-surface-variant text-label-sm">{t('extensions.mcp.loading')}</div>
      ) : servers.length === 0 ? (
        <div className="text-center py-md text-on-surface-variant text-label-sm">
          {t('extensions.mcp.noMcpServers')}
        </div>
      ) : (
        <div className="border border-outline-variant/30 rounded-2xl overflow-hidden bg-surface-container-lowest/50">
          {servers.map((srv, i) => (
            <div
              key={srv.name}
              className={`flex items-center gap-md px-md py-sm ${i === servers.length - 1 ? "" : "border-b border-outline-variant/15"}`}
            >
              <span className="material-symbols-outlined text-primary text-[20px]">cloud</span>
              <div className="flex-1 min-w-0">
                <div className="flex items-center gap-xs">
                  <div className="font-bold text-label-md text-on-surface truncate">{srv.name}</div>
                  <span
                    className={`text-label-xs px-xs py-[1px] rounded-full font-bold shrink-0 ${
                      srv.connected
                        ? "bg-primary-container/60 text-on-primary-container"
                        : "bg-surface-container-highest text-on-surface-variant"
                    }`}
                  >
                    {srv.connected
                      ? t("extensions.mcp.toolCount").replace("{count}", String(srv.tool_count))
                      : t("extensions.mcp.offline")}
                  </span>
                </div>
                {srv.command && (
                  <div className="text-label-xs text-on-surface-variant font-mono truncate">
                    {srv.command}
                  </div>
                )}
              </div>
              <button
                type="button"
                onClick={() => onUninstall(srv.name)}
                disabled={busyId === `uninstall:${srv.name}`}
                className="px-sm py-xs rounded-lg bg-error-container/40 text-on-error-container text-label-xs font-bold hover:bg-error-container/70 disabled:opacity-50"
              >
                {busyId === `uninstall:${srv.name}` ? "…" : t('extensions.mcp.remove')}
              </button>
            </div>
          ))}
        </div>
      )}
    </section>
  );
}
