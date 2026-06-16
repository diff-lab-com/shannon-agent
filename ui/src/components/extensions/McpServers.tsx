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
    setBusyId(server.id);
    setFeedback(null);
    try {
      // For P2: the backend resolver picks OAuth / .mcpb / stdio based on
      // registry metadata. The UI calls install_mcp_stdio as a fallback for
      // Tier-3 servers that lack a resolver — those need the manual form.
      setFeedback({
        id: server.id,
        msg: "Use the manual form below for Tier-3 stdio servers, or Featured tab for OAuth.",
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
      setFeedback({ id: `uninstall:${name}`, msg: `Removed ${name}`, ok: true });
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
  return (
    <section>
      <h3 className="text-label-lg font-bold text-on-surface-variant uppercase tracking-wide mb-sm">
        Registry · {servers.length}
      </h3>

      {loading && (
        <div className="text-center py-lg text-on-surface-variant">
          <span className="material-symbols-outlined animate-spin align-middle mr-xs">progress_activity</span>
          Fetching registry…
        </div>
      )}

      {error && (
        <div className="border border-error/30 rounded-xl p-md bg-error-container/10 text-label-sm text-error">
          Registry unavailable: <span className="font-mono">{error}</span>
        </div>
      )}

      {!loading && !error && servers.length === 0 && (
        <div className="text-center py-lg text-on-surface-variant text-label-md">
          No servers found.
        </div>
      )}

      {!loading && !error && servers.length > 0 && (
        <div className="grid grid-cols-1 md:grid-cols-2 gap-md">
          {servers.map((server) => {
            const isBusy = busyId === server.id;
            const isInstalled = installedNames.has(server.name);
            const serverFeedback = feedback?.id === server.id ? feedback : null;
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
                      Repo
                    </a>
                  )}
                  <button
                    type="button"
                    onClick={() => onInstall(server)}
                    disabled={isBusy || isInstalled}
                    className="px-sm py-xs rounded-lg bg-primary text-on-primary text-label-xs font-bold hover:bg-primary/90 disabled:opacity-50 disabled:cursor-not-allowed"
                  >
                    {isBusy ? "…" : isInstalled ? "Installed" : "Install"}
                  </button>
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
      onError("Enter a server name first.");
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
      <h3 className="text-label-lg font-bold text-on-surface mb-xs">Upload .mcpb bundle</h3>
      <p className="text-label-sm text-on-surface-variant mb-sm">
        Install an MCP server from a <code>.mcpb</code> ZIP archive on disk.
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
          Extracting & installing…
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
      onError("Server name and command are required.");
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
    <section className="border border-outline-variant/30 rounded-2xl p-md bg-surface-container-low/30">
      <h3 className="text-label-lg font-bold text-on-surface mb-xs">{t('extensions.mcp.addStdioTitle')}</h3>
      <p className="text-label-sm text-on-surface-variant mb-sm">
        Tier-3 escape hatch: specify command, args, and env directly.
      </p>
      <div className="space-y-sm">
        <label className="block">
          <span className="block text-label-xs text-on-surface-variant mb-[2px]">Server name *</span>
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
          <span className="block text-label-xs text-on-surface-variant mb-[2px]">Command *</span>
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
          <span className="block text-label-xs text-on-surface-variant mb-[2px]">Args (space-separated, quotes allowed)</span>
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
          <span className="block text-label-xs text-on-surface-variant mb-[2px]">Env (KEY=value, one per line)</span>
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
              Installing…
            </>
          ) : (
            <>
              <span className="material-symbols-outlined text-[16px]">add</span>
              Install
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
  return (
    <section>
      <h3 className="text-label-lg font-bold text-on-surface-variant uppercase tracking-wide mb-sm">
        Installed · {servers.length}
      </h3>
      {loading ? (
        <div className="text-center py-md text-on-surface-variant text-label-sm">Loading…</div>
      ) : servers.length === 0 ? (
        <div className="text-center py-md text-on-surface-variant text-label-sm">
          No MCP servers configured.
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
                <div className="font-bold text-label-md text-on-surface truncate">{srv.name}</div>
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
                {busyId === `uninstall:${srv.name}` ? "…" : "Remove"}
              </button>
            </div>
          ))}
        </div>
      )}
    </section>
  );
}
