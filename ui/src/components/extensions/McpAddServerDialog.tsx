// McpAddServerDialog — modal hosting the three install flows for MCP servers.
//
// Tabs:
//   1. Search — debounced search over the MCP registry. One-click install
//      via buildSpecFromPackage + installMcpStdio (existing flow, moved here
//      verbatim).
//   2. Paste JSON — textarea accepting Cursor (`{ "mcpServers": { ... } }`)
//      or Claude Desktop / single-server shape. Parsed client-side, one
//      install_mcp_stdio call per server.
//   3. Manual — the legacy stdio form (name + command + args + env).
//
// Modal pattern mirrors InstallDialog.tsx: fixed overlay, Escape to close,
// click backdrop to close. The parent owns the installed list; we accept
// `onInstalled` and call it after every successful install so the page
// refreshes.

import { useEffect, useMemo, useRef, useState } from "react";
import { useIntl } from "react-intl";
import { toast } from "sonner";
import {
  listMcpRegistryServers,
  installMcpStdio,
  type RegistryServer,
  type StdioMcpSpecPayload,
} from "@/lib/tauri-api";
import {
  isValidPackageName,
  isValidVersion,
  safeErrorMessage,
} from "@/lib/packageValidation";
import { useModalFocus } from "@/hooks/useModalFocus";
import LoadingState from "@/components/ui/loading-state";

export interface McpAddServerDialogProps {
  open: boolean;
  onClose: () => void;
  onInstalled: () => void;
  installedNames: Set<string>;
  /** Initial query seeded from the Extensions shell's shared search box. */
  initialQuery?: string;
}

// --- Registry package metadata (not yet on the shared TS interface) ------

interface RegistryPackage {
  kind: string; // "npm" | "pip" | "docker" | ...
  name?: string;
  registry_url?: string;
  version?: string;
}

type RegistryServerWithPackage = RegistryServer & {
  package?: RegistryPackage | null;
};

// --- Shared helpers (kept identical to the prior McpServers.tsx) ---------

function buildSpecFromPackage(
  serverName: string,
  pkg: RegistryPackage | null | undefined,
): StdioMcpSpecPayload | null {
  if (!pkg) return null;
  const name = pkg.name?.trim();
  const version = pkg.version?.trim();
  const versionSuffix = version ? `@${version}` : "";
  switch (pkg.kind) {
    case "npm": {
      if (!name || !isValidPackageName("npm", name)) return null;
      if (version && !isValidVersion("npm", version)) return null;
      return {
        server_name: serverName,
        command: "npx",
        args: ["-y", versionSuffix ? `${name}${versionSuffix}` : name],
        env: [],
      };
    }
    case "pip": {
      if (!name || !isValidPackageName("pip", name)) return null;
      return {
        server_name: serverName,
        command: "uvx",
        args: [name],
        env: [],
      };
    }
    case "docker": {
      if (!name || !isValidPackageName("docker", name)) return null;
      if (version && !isValidVersion("docker", version)) return null;
      return {
        server_name: serverName,
        command: "docker",
        args: ["run", "-i", "--rm", name],
        env: [],
      };
    }
    default:
      return null;
  }
}

function packageManagerLabel(
  pkg: RegistryPackage | null | undefined,
): string | null {
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

// --- JSON paste parsing ----------------------------------------------------

interface ParsedMcpServer {
  name: string;
  command: string;
  args: string[];
  env: [string, string][];
}

/**
 * Parse pasted JSON in either Cursor (`{ mcpServers: { ... } }`) or
 * single-server shape (`{ command, args?, env? }`). Returns a list of
 * servers to install. Throws on malformed input or missing `command`.
 */
function parseMcpJson(raw: string): ParsedMcpServer[] {
  const trimmed = raw.trim();
  if (!trimmed) throw new Error("empty input");
  let data: unknown;
  try {
    data = JSON.parse(trimmed);
  } catch (e) {
    throw new Error(e instanceof Error ? e.message : "invalid JSON");
  }
  if (typeof data !== "object" || data === null) {
    throw new Error("JSON root must be an object");
  }

  // Helper: normalise a single server definition into ParsedMcpServer.
  const normalise = (
    name: string,
    def: unknown,
    fallbackName?: string,
  ): ParsedMcpServer | null => {
    if (typeof def !== "object" || def === null) return null;
    const obj = def as Record<string, unknown>;
    const command = typeof obj.command === "string" ? obj.command.trim() : "";
    if (!command) return null;
    const serverName = (name || fallbackName || "Custom Server").trim();
    const args = Array.isArray(obj.args)
      ? obj.args.filter((a) => typeof a === "string").map((a) => String(a))
      : [];
    const env: [string, string][] = [];
    if (obj.env && typeof obj.env === "object") {
      for (const [k, v] of Object.entries(obj.env as Record<string, unknown>)) {
        if (typeof v === "string" || typeof v === "number") {
          env.push([k, String(v)]);
        }
      }
    }
    return { name: serverName, command, args, env };
  };

  const root = data as Record<string, unknown>;

  // Cursor format: { "mcpServers": { "name": { ... }, ... } }
  if (root.mcpServers && typeof root.mcpServers === "object") {
    const entries = Object.entries(root.mcpServers as Record<string, unknown>);
    const out: ParsedMcpServer[] = [];
    for (const [serverName, def] of entries) {
      const parsed = normalise(serverName, def);
      if (parsed) out.push(parsed);
    }
    if (out.length === 0) {
      throw new Error("no valid servers in mcpServers");
    }
    return out;
  }

  // Claude Desktop format: { "mcpServers": { ... } } is the same as Cursor,
  // but Claude Desktop config files sometimes nest under a different key or
  // present a single server at the root. Handle single-server-at-root too.
  if (typeof root.command === "string") {
    const parsed = normalise("", root, "Custom Server");
    if (!parsed) throw new Error("missing command");
    return [parsed];
  }

  throw new Error("unrecognized JSON shape");
}

// ===========================================================================
// Component
// ===========================================================================

type TabKey = "search" | "paste" | "manual";

export default function McpAddServerDialog({
  open,
  onClose,
  onInstalled,
  installedNames,
  initialQuery = "",
}: McpAddServerDialogProps) {
  const intl = useIntl();
  const t = (id: string, values?: Record<string, string | number>) =>
    intl.formatMessage({ id }, values);

  const [tab, setTab] = useState<TabKey>("search");

  const modalRef = useRef<HTMLDivElement>(null);
  useModalFocus(open, modalRef);

  // Reset to the first tab whenever the dialog is opened.
  useEffect(() => {
    if (open) setTab("search");
  }, [open]);

  // Escape closes the dialog (mirrors InstallDialog.tsx).
  useEffect(() => {
    if (!open) return;
    const handler = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [open, onClose]);

  if (!open) return null;

  const tabs: { key: TabKey; label: string }[] = [
    { key: "search", label: t("extensions.mcp.addDialog.tab.search") },
    { key: "paste", label: t("extensions.mcp.addDialog.tab.paste") },
    { key: "manual", label: t("extensions.mcp.addDialog.tab.manual") },
  ];

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/40 p-md"
      role="dialog"
      aria-modal="true"
      aria-labelledby="mcp-add-dialog-title"
      onClick={onClose}
    >
      <div
        ref={modalRef}
        onClick={(e) => e.stopPropagation()}
        className="bg-surface-container-lowest rounded-2xl shadow-2xl border border-outline-variant/40 w-full max-w-3xl max-h-[90vh] overflow-y-auto p-lg flex flex-col gap-md"
      >
        <div className="flex items-center justify-between">
          <h2
            id="mcp-add-dialog-title"
            className="font-headline-md text-[18px] font-bold text-on-surface flex items-center gap-sm"
          >
            <span className="material-symbols-outlined icon-md text-primary">
              add_circle
            </span>
            {t("extensions.mcp.addDialog.title")}
          </h2>
          <button
            type="button"
            onClick={onClose}
            aria-label={t("extensions.installDialog.closeAria")}
            className="text-on-surface-variant hover:bg-surface-container-high rounded-full p-xs cursor-pointer focus:outline-none focus-visible:ring-2 focus-visible:ring-primary/30"
          >
            <span className="material-symbols-outlined text-[18px]">close</span>
          </button>
        </div>

        {/* Tab strip */}
        <div
          role="tablist"
          aria-label={t("extensions.mcp.addDialog.title")}
          className="flex gap-xs border-b border-outline-variant/30"
        >
          {tabs.map((tb) => (
            <button
              key={tb.key}
              role="tab"
              type="button"
              aria-selected={tab === tb.key}
              onClick={() => setTab(tb.key)}
              className={`px-md py-sm text-label-sm font-bold border-b-2 -mb-px transition-colors cursor-pointer ${
                tab === tb.key
                  ? "border-primary text-primary"
                  : "border-transparent text-on-surface-variant hover:text-on-surface"
              }`}
            >
              {tb.label}
            </button>
          ))}
        </div>

        {tab === "search" && (
          <SearchTab
            installedNames={installedNames}
            initialQuery={initialQuery}
            onInstalled={onInstalled}
          />
        )}
        {tab === "paste" && (
          <PasteJsonTab onInstalled={onInstalled} />
        )}
        {tab === "manual" && (
          <ManualTab onInstalled={onInstalled} />
        )}
      </div>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Tab 1: Search the registry
// ---------------------------------------------------------------------------

function SearchTab({
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
                <button
                  type="button"
                  onClick={() => handleInstall(server)}
                  disabled={isBusy || isInstalled}
                  className="shrink-0 px-sm py-xs rounded-lg bg-primary text-on-primary text-label-xs font-bold hover:bg-primary/90 disabled:opacity-50 disabled:cursor-not-allowed"
                >
                  {isBusy
                    ? "…"
                    : isInstalled
                      ? t("extensions.mcp.installed")
                      : t("extensions.mcp.install")}
                </button>
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}

// ---------------------------------------------------------------------------
// Tab 2: Paste JSON
// ---------------------------------------------------------------------------

function PasteJsonTab({ onInstalled }: { onInstalled: () => void }) {
  const intl = useIntl();
  const t = (id: string, values?: Record<string, string | number>) =>
    intl.formatMessage({ id }, values);

  const [text, setText] = useState("");
  const [parsed, setParsed] = useState<ParsedMcpServer[] | null>(null);
  const [parseError, setParseError] = useState<string | null>(null);
  const [installing, setInstalling] = useState(false);

  function handleParse(raw: string) {
    setText(raw);
    if (!raw.trim()) {
      setParsed(null);
      setParseError(null);
      return;
    }
    try {
      const servers = parseMcpJson(raw);
      setParsed(servers);
      setParseError(null);
    } catch (e) {
      setParsed(null);
      setParseError(safeErrorMessage(e, "parse failed"));
    }
  }

  async function handleInstallAll() {
    if (!parsed || parsed.length === 0) return;
    setInstalling(true);
    let ok = 0;
    let failed = 0;
    for (const srv of parsed) {
      try {
        await installMcpStdio({
          server_name: srv.name,
          command: srv.command,
          args: srv.args,
          env: srv.env,
        });
        ok++;
      } catch {
        failed++;
      }
    }
    setInstalling(false);
    if (ok > 0) {
      toast.success(
        intl.formatMessage(
          { id: "extensions.mcp.oneClick.installSuccessCount" },
          { count: ok },
        ),
      );
      onInstalled();
    }
    if (failed > 0) {
      toast.error(
        intl.formatMessage(
          { id: "extensions.mcp.oneClick.installFailedCount" },
          { count: failed },
        ),
      );
    }
  }

  return (
    <div className="flex flex-col gap-sm" role="tabpanel">
      <textarea
        value={text}
        onChange={(e) => handleParse(e.target.value)}
        placeholder={t("extensions.mcp.addDialog.paste.placeholder")}
        aria-label={t("extensions.mcp.addDialog.paste.placeholder")}
        rows={10}
        className="w-full px-md py-sm rounded-lg border border-outline-variant/40 bg-surface text-label-sm font-mono focus:outline-none focus-visible:ring-2 focus-visible:ring-primary/30"
        spellCheck={false}
      />

      {parseError && (
        <div className="border border-error/30 rounded-xl p-sm bg-error-container/10 text-label-xs text-error">
          {t("extensions.mcp.addDialog.paste.parseError", {
            error: parseError,
          })}
        </div>
      )}

      {parsed && parsed.length > 0 && (
        <div className="flex items-center justify-between gap-sm">
          <ul className="flex-1 min-w-0 text-label-xs text-on-surface-variant list-disc pl-md">
            {parsed.map((s, i) => (
              <li key={`${s.name}-${i}`} className="truncate">
                <span className="font-mono">{s.name}</span> —{" "}
                <span className="font-mono">{s.command}</span>
              </li>
            ))}
          </ul>
          <button
            type="button"
            onClick={handleInstallAll}
            disabled={installing}
            className="shrink-0 px-md py-sm rounded-lg bg-primary text-on-primary text-label-sm font-bold hover:bg-primary/90 disabled:opacity-50 inline-flex items-center gap-xs cursor-pointer"
          >
            <span className="material-symbols-outlined icon-sm">
              {installing ? "progress_activity" : "download"}
            </span>
            {t("extensions.mcp.addDialog.paste.install", { count: parsed.length })}
          </button>
        </div>
      )}
    </div>
  );
}

// ---------------------------------------------------------------------------
// Tab 3: Manual stdio form (moved as-is from the prior McpServers.tsx)
// ---------------------------------------------------------------------------

function parseArgs(text: string): string[] {
  const out: string[] = [];
  const re = /"([^"]*)"|'([^']*)'|(\S+)/g;
  let m: RegExpExecArray | null;
  while ((m = re.exec(text)) !== null) {
    out.push(m[1] ?? m[2] ?? m[3] ?? "");
  }
  return out;
}

function parseEnv(text: string): [string, string][] {
  const out: [string, string][] = [];
  for (const rawLine of text.split(/\n/)) {
    const line = rawLine.trim();
    if (!line) continue;
    const eq = line.indexOf("=");
    const key = (eq < 0 ? line : line.slice(0, eq)).trim();
    const val = eq < 0 ? "" : line.slice(eq + 1).trim();
    if (!/^[a-zA-Z_][a-zA-Z0-9_]*$/.test(key)) continue;
    out.push([key, val]);
  }
  return out;
}

function ManualTab({ onInstalled }: { onInstalled: () => void }) {
  const intl = useIntl();
  const t = (id: string, values?: Record<string, string | number>) =>
    intl.formatMessage({ id }, values);

  const [name, setName] = useState("");
  const [command, setCommand] = useState("");
  const [argsText, setArgsText] = useState("");
  const [envText, setEnvText] = useState("");
  const [busy, setBusy] = useState(false);

  async function handleSubmit() {
    if (!name.trim() || !command.trim()) {
      toast.error(t("extensions.mcp.needNameAndCommand"));
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
      await installMcpStdio(spec);
      toast.success(
        t("extensions.mcp.oneClick.installSuccess", { name: name.trim() }),
      );
      onInstalled();
      setName("");
      setCommand("");
      setArgsText("");
      setEnvText("");
    } catch (err) {
      toast.error(
        t("extensions.mcp.oneClick.installFailed", {
          error: safeErrorMessage(err, "install failed"),
        }),
      );
    } finally {
      setBusy(false);
    }
  }

  return (
    <div
      className="flex flex-col gap-sm"
      role="tabpanel"
      aria-label={t("extensions.mcp.addDialog.manual.aria")}
    >
      <p className="text-label-sm text-on-surface-variant">
        {t("extensions.mcp.manualDesc")}
      </p>
      <label className="block">
        <span className="block text-label-xs text-on-surface-variant mb-[2px]">
          {t("extensions.mcp.serverNameRequired")}
        </span>
        <input
          type="text"
          value={name}
          onChange={(e) => setName(e.target.value)}
          placeholder={t('extensions.mcp.serverName.placeholder')}
          className="w-full px-sm py-xs rounded border border-outline-variant text-label-sm bg-surface"
          disabled={busy}
        />
      </label>
      <label className="block">
        <span className="block text-label-xs text-on-surface-variant mb-[2px]">
          {t("extensions.mcp.commandRequired")}
        </span>
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
        <span className="block text-label-xs text-on-surface-variant mb-[2px]">
          {t("extensions.mcp.argsLabel")}
        </span>
        <input
          type="text"
          value={argsText}
          onChange={(e) => setArgsText(e.target.value)}
          placeholder="-y @modelcontextprotocol/server-filesystem /tmp"
          className="w-full px-sm py-xs rounded border border-outline-variant text-label-sm bg-surface font-mono"
          disabled={busy}
        />
      </label>
      <label className="block">
        <span className="block text-label-xs text-on-surface-variant mb-[2px]">
          {t("extensions.mcp.envLabel")}
        </span>
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
        className="inline-flex items-center gap-xs px-md py-sm rounded-lg bg-primary text-on-primary text-label-sm font-bold hover:bg-primary/90 disabled:opacity-50 cursor-pointer"
      >
        <span className="material-symbols-outlined icon-sm">
          {busy ? "progress_activity" : "add"}
        </span>
        {busy
          ? t("extensions.mcp.installing")
          : t("extensions.mcp.install")}
      </button>
    </div>
  );
}
