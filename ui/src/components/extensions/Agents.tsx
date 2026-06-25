import { useEffect, useState } from "react";
import { useOutletContext } from "react-router-dom";
import { useIntl } from 'react-intl'
import {
  listAgentCatalog,
  listInstalledAgentPlugins,
  installNativeAgent,
  installAgentFromRepo,
  uninstallAgentPlugin,
  type AgentCatalogEntry,
  type InstalledAgent,
} from "@/lib/tauri-api";
import { SecurityBadge } from "./SecurityBadge";
import { ConfirmDialog } from "@/components/ui/confirm-dialog";

/**
 * P4 Agents tab — federated catalog + install/remove.
 *
 * Lists agents from native built-ins + community GitHub upstreams
 * (VoltAgent/awesome-claude-code-agents, rohitg00/claude-code-agents).
 * Each entry has an Install button that either:
 * - Native: writes agent.md directly via install_native_agent
 * - GitHub: git clones into ~/.shannon/agents/<plugin>/ via install_agent_from_repo
 */
export default function Agents() {
  const intl = useIntl()
  const t = (id: string, values?: Record<string, string | number>) => intl.formatMessage({ id }, values)

  const { search } = useOutletContext<{ search: string }>();

  const [catalog, setCatalog] = useState<AgentCatalogEntry[]>([]);
  const [catalogLoading, setCatalogLoading] = useState(true);
  const [catalogError, setCatalogError] = useState<string | null>(null);

  const [installed, setInstalled] = useState<InstalledAgent[]>([]);
  const [installedLoading, setInstalledLoading] = useState(true);

  const [busyId, setBusyId] = useState<string | null>(null);
  const [feedback, setFeedback] = useState<{ id: string; msg: string; ok: boolean } | null>(null);
  const [removeTarget, setRemoveTarget] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    setCatalogLoading(true);
    listAgentCatalog()
      .then((rows) => {
        if (!cancelled) {
          setCatalog(rows);
          setCatalogError(null);
        }
      })
      .catch((err) => {
        if (!cancelled) setCatalogError(String(err));
      })
      .finally(() => {
        if (!cancelled) setCatalogLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, []);

  const refreshInstalled = () => {
    listInstalledAgentPlugins()
      .then(setInstalled)
      .finally(() => setInstalledLoading(false));
  };

  useEffect(() => {
    refreshInstalled();
  }, []);

  async function handleInstall(entry: AgentCatalogEntry) {
    setBusyId(entry.id);
    setFeedback(null);
    try {
      if (entry.source.type === 'native') {
        const model = (entry.metadata.model as string | undefined) ?? 'claude-sonnet-4-6';
        const tools = Array.isArray(entry.metadata.tools) ? entry.metadata.tools : [];
        const toolsYaml = tools.length > 0 ? `\ntools: [${tools.join(', ')}]` : '';
        const body = `---\nname: ${entry.name}\ndescription: ${entry.description}\nmodel: ${model}${toolsYaml}\n---\n# ${entry.name}\n\n${entry.description}\n`;
        await installNativeAgent(entry.name, body);
      } else if (entry.source.type === 'git_hub_repo') {
        const repo = entry.source.repo;
        const ref_ = entry.source.ref_ ?? 'main';
        await installAgentFromRepo(entry.name, repo, ref_);
      } else {
        setFeedback({ id: entry.id, msg: `Unsupported agent source: ${entry.source.type}`, ok: false });
        return;
      }
      setFeedback({ id: entry.id, msg: `Installed ${entry.name}`, ok: true });
      refreshInstalled();
    } catch (err) {
      setFeedback({ id: entry.id, msg: String(err), ok: false });
    } finally {
      setBusyId(null);
    }
  }

  async function handleUninstall(name: string) {
    setBusyId(`uninstall:${name}`);
    setFeedback(null);
    try {
      await uninstallAgentPlugin(name);
      setFeedback({ id: `uninstall:${name}`, msg: `Removed ${name}`, ok: true });
      refreshInstalled();
    } catch (err) {
      setFeedback({ id: `uninstall:${name}`, msg: String(err), ok: false });
    } finally {
      setBusyId(null);
    }
  }

  const installedNames = new Set(installed.map((a) => a.name));
  const filtered = search
    ? catalog.filter(
        (e) =>
          e.name.toLowerCase().includes(search.toLowerCase()) ||
          e.description.toLowerCase().includes(search.toLowerCase()) ||
          (e.author ?? '').toLowerCase().includes(search.toLowerCase())
      )
    : catalog;

  return (
    <div className="p-lg max-w-6xl mx-auto space-y-xl">
      <header>
        <h2 className="text-headline-md font-bold text-on-surface mb-xs">{t('extensions.agents.title')}</h2>
        <p className="text-body-md text-on-surface-variant">
          {t('extensions.agents.subtitle')}
        </p>
      </header>

      {catalogLoading && (
        <div className="text-center py-lg text-on-surface-variant">
          <span className="material-symbols-outlined animate-spin align-middle mr-xs">progress_activity</span>
          {t('extensions.agents.loading')}
        </div>
      )}

      {catalogError && (
        <div className="border border-error/30 rounded-xl p-md bg-error-container/10 text-label-sm text-error">
          {t('extensions.agents.loadError')}: <span className="font-mono">{catalogError}</span>
        </div>
      )}

      {!catalogLoading && !catalogError && (
        <section>
          <h3 className="text-label-lg font-bold text-on-surface-variant uppercase tracking-wide mb-sm">
            Catalog · {filtered.length}
          </h3>
          {filtered.length === 0 ? (
            <div className="text-center py-md text-on-surface-variant text-label-md">
              {t('extensions.agents.noAgents')}
            </div>
          ) : (
            <div className="grid grid-cols-1 md:grid-cols-2 gap-md">
              {filtered.map((entry) => (
                <AgentCard
                  key={entry.id}
                  entry={entry}
                  installed={installedNames.has(entry.name)}
                  busy={busyId === entry.id}
                  feedback={feedback?.id === entry.id ? feedback : null}
                  onInstall={() => handleInstall(entry)}
                />
              ))}
            </div>
          )}
        </section>
      )}

      <section>
        <h3 className="text-label-lg font-bold text-on-surface-variant uppercase tracking-wide mb-sm">
          Installed · {installed.length}
        </h3>
        {installedLoading ? (
          <div className="text-center py-md text-on-surface-variant text-label-sm">{t('extensions.agents.loadingInstalled')}</div>
        ) : installed.length === 0 ? (
          <div className="text-center py-md text-on-surface-variant text-label-sm">
            {t('extensions.agents.noInstalled')}
          </div>
        ) : (
          <div className="border border-outline-variant/30 rounded-2xl overflow-hidden bg-surface-container-lowest/50">
            {installed.map((agent, i) => (
              <div
                key={agent.name}
                className={`flex items-center gap-md px-md py-sm ${i === installed.length - 1 ? "" : "border-b border-outline-variant/15"}`}
              >
                <span className="material-symbols-outlined text-primary text-[20px]">smart_toy</span>
                <div className="flex-1 min-w-0">
                  <div className="font-bold text-label-md text-on-surface truncate">{agent.name}</div>
                  <div className="text-label-xs text-on-surface-variant font-mono truncate">
                    {agent.path}
                  </div>
                </div>
                <button
                  type="button"
                  onClick={() => setRemoveTarget(agent.name)}
                  disabled={busyId === `uninstall:${agent.name}`}
                  className="px-sm py-xs rounded-lg bg-error-container/40 text-on-error-container text-label-xs font-bold hover:bg-error-container/70 disabled:opacity-50"
                >
                  {busyId === `uninstall:${agent.name}` ? "…" : t('extensions.agents.remove')}
                </button>
              </div>
            ))}
          </div>
        )}
      </section>

      <ConfirmDialog
        open={removeTarget !== null}
        title={t('extensions.agents.removeConfirm.title')}
        message={t('extensions.agents.removeConfirm.message', { name: removeTarget ?? '' })}
        confirmLabel={t('extensions.agents.removeConfirm.confirm')}
        cancelLabel={t('extensions.agents.removeConfirm.cancel')}
        destructive
        busy={busyId?.startsWith('uninstall:') ?? false}
        onConfirm={() => {
          if (removeTarget) void handleUninstall(removeTarget).finally(() => setRemoveTarget(null))
        }}
        onCancel={() => setRemoveTarget(null)}
      />
    </div>
  );
}

function AgentCard({
  entry,
  installed,
  busy,
  feedback,
  onInstall,
}: {
  entry: AgentCatalogEntry;
  installed: boolean;
  busy: boolean;
  feedback: { id: string; msg: string; ok: boolean } | null;
  onInstall: () => void;
}) {
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })

  const trustLabel = TRUST_LABELS[entry.trust];
  const model = (entry.metadata.model as string | undefined) ?? null;
  const tools = Array.isArray(entry.metadata.tools) ? entry.metadata.tools : [];
  return (
    <div className="border border-outline-variant/30 rounded-2xl p-md bg-surface-container-low/40 flex flex-col">
      <div className="flex items-start justify-between mb-xs gap-xs">
        <h4 className="font-bold text-label-md text-on-surface">{entry.name}</h4>
        <div className="flex items-center gap-[4px] shrink-0">
          <SecurityBadge text={entry.description} trust={entry.trust} />
          <span className={`text-label-xs px-xs py-[1px] rounded-full font-bold ${trustLabel.cls}`}>
            {trustLabel.text}
          </span>
        </div>
      </div>
      <p className="text-label-sm text-on-surface-variant flex-1 mb-sm line-clamp-2">
        {entry.description}
      </p>
      {(model || tools.length > 0) && (
        <div className="text-label-xs text-on-surface-variant mb-xs font-mono">
          {model && <span>model: {model}</span>}
          {model && tools.length > 0 && <span> · </span>}
          {tools.length > 0 && <span>tools: {tools.join(', ')}</span>}
        </div>
      )}
      {entry.author && (
        <div className="text-label-xs text-on-surface-variant mb-xs font-mono">
          {entry.author}
          {entry.version && <span className="ml-xs">· v{entry.version}</span>}
        </div>
      )}
      {feedback && (
        <div className={`text-label-xs mb-xs ${feedback.ok ? "text-primary" : "text-error"}`}>
          {feedback.msg}
        </div>
      )}
      <div className="flex gap-xs">
        {entry.homepage_url && (
          <a
            href={entry.homepage_url}
            target="_blank"
            rel="noreferrer"
            className="px-sm py-xs rounded-lg bg-surface-container-high text-on-surface text-label-xs font-bold hover:bg-surface-container-highest"
          >
            {t('extensions.agents.view')}
          </a>
        )}
        <button
          type="button"
          onClick={onInstall}
          disabled={busy || installed}
          className="px-sm py-xs rounded-lg bg-primary text-on-primary text-label-xs font-bold hover:bg-primary/90 disabled:opacity-50 disabled:cursor-not-allowed"
        >
          {busy ? "…" : installed ? t('extensions.agents.installed') : t('extensions.agents.install')}
        </button>
      </div>
    </div>
  );
}

const TRUST_LABELS: Record<AgentCatalogEntry['trust'], { text: string; cls: string }> = {
  verified: { text: "Verified", cls: "bg-primary-container/50 text-on-primary-container" },
  official: { text: "Official", cls: "bg-secondary-container/50 text-on-secondary-container" },
  community: { text: "Community", cls: "bg-tertiary-container/50 text-on-tertiary-container" },
  unknown: { text: "Unknown", cls: "bg-surface-container-highest text-on-surface-variant" },
};

// Trust labels are static — no i18n needed for these constants
