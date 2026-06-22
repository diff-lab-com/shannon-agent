import { useEffect, useState } from "react";
import { useOutletContext } from "react-router-dom";
import { useIntl } from 'react-intl'
import {
  listSkillCatalog,
  listInstalledSkillPlugins,
  installNativeSkill,
  installSkillFromRepo,
  uninstallSkillPlugin,
  type SkillCatalogEntry,
  type InstalledSkill,
} from "@/lib/tauri-api";

/**
 * P3 Skills tab — federated catalog + install/remove.
 *
 * Lists skills from native built-ins + GitHub upstreams (anthropics/skills,
 * obra/superpowers). Each entry has an Install button that either:
 * - Native: writes SKILL.md directly via install_native_skill
 * - GitHub: git clones into ~/.shannon/skills/<plugin>/ via install_skill_from_repo
 *
 * Installed skill plugins show at the bottom with Remove buttons.
 */
export default function Skills() {
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })

  const { search } = useOutletContext<{ search: string }>();

  const [catalog, setCatalog] = useState<SkillCatalogEntry[]>([]);
  const [catalogLoading, setCatalogLoading] = useState(true);
  const [catalogError, setCatalogError] = useState<string | null>(null);

  const [installed, setInstalled] = useState<InstalledSkill[]>([]);
  const [installedLoading, setInstalledLoading] = useState(true);

  const [busyId, setBusyId] = useState<string | null>(null);
  const [feedback, setFeedback] = useState<{ id: string; msg: string; ok: boolean } | null>(null);

  useEffect(() => {
    let cancelled = false;
    setCatalogLoading(true);
    listSkillCatalog()
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
    listInstalledSkillPlugins()
      .then(setInstalled)
      .finally(() => setInstalledLoading(false));
  };

  useEffect(() => {
    refreshInstalled();
  }, []);

  async function handleInstall(entry: SkillCatalogEntry) {
    setBusyId(entry.id);
    setFeedback(null);
    try {
      if (entry.source.type === 'native') {
        // Built-in skill — write a stub SKILL.md using its description.
        const body = `---\nname: ${entry.name}\ndescription: ${entry.description}\n---\n# ${entry.name}\n\n${entry.description}\n`;
        await installNativeSkill(entry.name, body);
      } else if (entry.source.type === 'git_hub_repo') {
        const repo = entry.source.repo;
        const ref_ = entry.source.ref_ ?? 'main';
        await installSkillFromRepo(entry.name, repo, ref_);
      } else {
        setFeedback({ id: entry.id, msg: `Unsupported skill source: ${entry.source.type}`, ok: false });
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
      await uninstallSkillPlugin(name);
      setFeedback({ id: `uninstall:${name}`, msg: `Removed ${name}`, ok: true });
      refreshInstalled();
    } catch (err) {
      setFeedback({ id: `uninstall:${name}`, msg: String(err), ok: false });
    } finally {
      setBusyId(null);
    }
  }

  const installedNames = new Set(installed.map((s) => s.name));
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
        <h2 className="text-headline-md font-bold text-on-surface mb-xs">{t('extensions.skills.title')}</h2>
        <p className="text-body-md text-on-surface-variant">
          {t('extensions.skills.subtitle')}
        </p>
      </header>

      {catalogLoading && (
        <div className="text-center py-lg text-on-surface-variant">
          <span className="material-symbols-outlined animate-spin align-middle mr-xs">progress_activity</span>
          {t('extensions.skills.loading')}
        </div>
      )}

      {catalogError && (
        <div className="border border-error/30 rounded-xl p-md bg-error-container/10 text-label-sm text-error">
          {t('extensions.skills.loadError')}: <span className="font-mono">{catalogError}</span>
        </div>
      )}

      {!catalogLoading && !catalogError && (
        <>
          <section>
            <h3 className="text-label-lg font-bold text-on-surface-variant uppercase tracking-wide mb-sm">
              Catalog · {filtered.length}
            </h3>
            {filtered.length === 0 ? (
              <div className="text-center py-md text-on-surface-variant text-label-md">
                {t('extensions.skills.noSkills')}
              </div>
            ) : (
              <div className="grid grid-cols-1 md:grid-cols-2 gap-md">
                {filtered.map((entry) => (
                  <SkillCard
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
        </>
      )}

      <section>
        <h3 className="text-label-lg font-bold text-on-surface-variant uppercase tracking-wide mb-sm">
          Installed · {installed.length}
        </h3>
        {installedLoading ? (
          <div className="text-center py-md text-on-surface-variant text-label-sm">{t('extensions.skills.loadingInstalled')}</div>
        ) : installed.length === 0 ? (
          <div className="text-center py-md text-on-surface-variant text-label-sm">
            {t('extensions.skills.noInstalled')}
          </div>
        ) : (
          <div className="border border-outline-variant/30 rounded-2xl overflow-hidden bg-surface-container-lowest/50">
            {installed.map((skill, i) => (
              <div
                key={skill.name}
                className={`flex items-center gap-md px-md py-sm ${i === installed.length - 1 ? "" : "border-b border-outline-variant/15"}`}
              >
                <span className="material-symbols-outlined text-primary text-[20px]">extension</span>
                <div className="flex-1 min-w-0">
                  <div className="font-bold text-label-md text-on-surface truncate">{skill.name}</div>
                  <div className="text-label-xs text-on-surface-variant font-mono truncate">
                    {skill.path}
                  </div>
                </div>
                <button
                  type="button"
                  onClick={() => handleUninstall(skill.name)}
                  disabled={busyId === `uninstall:${skill.name}`}
                  className="px-sm py-xs rounded-lg bg-error-container/40 text-on-error-container text-label-xs font-bold hover:bg-error-container/70 disabled:opacity-50"
                >
                  {busyId === `uninstall:${skill.name}` ? "…" : t('extensions.skills.remove')}
                </button>
              </div>
            ))}
          </div>
        )}
      </section>
    </div>
  );
}

function SkillCard({
  entry,
  installed,
  busy,
  feedback,
  onInstall,
}: {
  entry: SkillCatalogEntry;
  installed: boolean;
  busy: boolean;
  feedback: { id: string; msg: string; ok: boolean } | null;
  onInstall: () => void;
}) {
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })

  const trustLabel = TRUST_LABELS[entry.trust];
  return (
    <div className="border border-outline-variant/30 rounded-2xl p-md bg-surface-container-low/40 flex flex-col">
      <div className="flex items-start justify-between mb-xs">
        <h4 className="font-bold text-label-md text-on-surface">{entry.name}</h4>
        <span className={`text-label-xs px-xs py-[1px] rounded-full font-bold ${trustLabel.cls}`}>
          {trustLabel.text}
        </span>
      </div>
      <p className="text-label-sm text-on-surface-variant flex-1 mb-sm line-clamp-2">
        {entry.description}
      </p>
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
            {t('extensions.skills.view')}
          </a>
        )}
        <button
          type="button"
          onClick={onInstall}
          disabled={busy || installed}
          className="px-sm py-xs rounded-lg bg-primary text-on-primary text-label-xs font-bold hover:bg-primary/90 disabled:opacity-50 disabled:cursor-not-allowed"
        >
          {busy ? "…" : installed ? t('extensions.skills.installed') : t('extensions.skills.install')}
        </button>
      </div>
    </div>
  );
}

const TRUST_LABELS: Record<SkillCatalogEntry['trust'], { text: string; cls: string }> = {
  verified: { text: "Verified", cls: "bg-primary-container/50 text-on-primary-container" },
  official: { text: "Official", cls: "bg-secondary-container/50 text-on-secondary-container" },
  community: { text: "Community", cls: "bg-tertiary-container/50 text-on-tertiary-container" },
  unknown: { text: "Unknown", cls: "bg-surface-container-highest text-on-surface-variant" },
};

// Trust labels are static — no i18n needed for these constants
