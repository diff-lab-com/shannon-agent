import { useEffect, useState } from "react";
import { useOutletContext } from "react-router-dom";
import { useIntl } from 'react-intl'
import {
  listDataSourceCatalog,
  listInstalledDataSources,
  installDataSource,
  uninstallDataSource,
  type DataSourceCatalogEntry,
  type DataSourceField,
  type InstalledDataSource,
} from "@/lib/tauri-api";
import DataSourcesQuery from "./DataSourcesQuery";

/**
 * P5 Data Sources tab — Tier-1 native Rust adapters.
 *
 * Unlike MCP/Skills/Agents tabs, this is a config-only install: there's no
 * upstream fetch. Each adapter declares required form fields in its catalog
 * metadata; we render those dynamically, persist them via install_data_source,
 * and the adapter loads the config file at query time.
 *
 * Two adapters ship today: Obsidian Vault, Email (IMAP).
 */
export default function DataSources() {
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })

  const { search } = useOutletContext<{ search: string }>();

  const [activeTab, setActiveTab] = useState<'adapters' | 'query'>('adapters');

  const [catalog, setCatalog] = useState<DataSourceCatalogEntry[]>([]);
  const [catalogLoading, setCatalogLoading] = useState(true);

  const [installed, setInstalled] = useState<InstalledDataSource[]>([]);
  const [installedLoading, setInstalledLoading] = useState(true);

  const [installingSlug, setInstallingSlug] = useState<string | null>(null);
  const [installForm, setInstallForm] = useState<Record<string, string>>({});
  const [feedback, setFeedback] = useState<{ slug: string; msg: string; ok: boolean } | null>(null);
  const [busySlug, setBusySlug] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    setCatalogLoading(true);
    listDataSourceCatalog()
      .then((rows) => {
        if (!cancelled) setCatalog(rows);
      })
      .finally(() => {
        if (!cancelled) setCatalogLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, []);

  const refreshInstalled = () => {
    listInstalledDataSources()
      .then(setInstalled)
      .finally(() => setInstalledLoading(false));
  };

  useEffect(() => {
    refreshInstalled();
  }, []);

  function startInstall(entry: DataSourceCatalogEntry) {
    const fields = entry.metadata.fields ?? [];
    const initial: Record<string, string> = {};
    for (const field of fields) {
      initial[field.key] = field.placeholder ?? "";
    }
    setInstallForm(initial);
    setInstallingSlug(entry.id);
    setFeedback(null);
  }

  async function submitInstall(entry: DataSourceCatalogEntry) {
    const fields = entry.metadata.fields ?? [];
    const config: Record<string, string> = {};
    for (const field of fields) {
      const value = (installForm[field.key] ?? "").trim();
      if (field.required && !value) {
        setFeedback({
          slug: entry.id,
          msg: `${field.label} is required`,
          ok: false,
        });
        return;
      }
      if (value) {
        config[field.key] = value;
      }
    }
    setBusySlug(entry.id);
    setFeedback(null);
    try {
      await installDataSource(
        entrySlug(entry.id),
        (entry.metadata.kind ?? "unknown").toString(),
        entry.name,
        config,
      );
      setFeedback({ slug: entry.id, msg: `Installed ${entry.name}`, ok: true });
      setInstallingSlug(null);
      refreshInstalled();
    } catch (err) {
      setFeedback({ slug: entry.id, msg: String(err), ok: false });
    } finally {
      setBusySlug(null);
    }
  }

  async function handleUninstall(slug: string) {
    setBusySlug(`uninstall:${slug}`);
    setFeedback(null);
    try {
      await uninstallDataSource(slug);
      setFeedback({ slug: `uninstall:${slug}`, msg: `Removed ${slug}`, ok: true });
      refreshInstalled();
    } catch (err) {
      setFeedback({ slug: `uninstall:${slug}`, msg: String(err), ok: false });
    } finally {
      setBusySlug(null);
    }
  }

  const installedSlugs = new Set(installed.map((row) => row.slug));
  const filtered = search
    ? catalog.filter(
        (e) =>
          e.name.toLowerCase().includes(search.toLowerCase()) ||
          e.description.toLowerCase().includes(search.toLowerCase()),
      )
    : catalog;

  return (
    <div className="p-lg max-w-5xl mx-auto space-y-xl">
      <header>
        <h2 className="text-headline-md font-bold text-on-surface mb-xs">{t('extensions.datasources.title')}</h2>
        <p className="text-body-md text-on-surface-variant">
          {t('extensions.datasources.subtitle')}
        </p>
      </header>

      <div className="flex gap-md border-b border-outline-variant/30">
        <button
          type="button"
          onClick={() => setActiveTab('adapters')}
          className={`px-md py-sm font-bold text-label-md transition-colors ${
            activeTab === 'adapters'
              ? 'text-primary border-b-2 border-primary'
              : 'text-on-surface-variant hover:text-on-surface'
          }`}
        >
          {t('extensions.datasources.tab.adapters')}
        </button>
        <button
          type="button"
          onClick={() => setActiveTab('query')}
          className={`px-md py-sm font-bold text-label-md transition-colors ${
            activeTab === 'query'
              ? 'text-primary border-b-2 border-primary'
              : 'text-on-surface-variant hover:text-on-surface'
          }`}
        >
          {t('extensions.datasources.tab.query')}
        </button>
      </div>

      {activeTab === 'query' ? (
        <DataSourcesQuery />
      ) : catalogLoading ? (
        <div className="text-center py-lg text-on-surface-variant">
          <span className="material-symbols-outlined animate-spin align-middle mr-xs">progress_activity</span>
          {t('extensions.datasources.loading')}
        </div>
      ) : (
        <section>
          <h3 className="text-label-lg font-bold text-on-surface-variant uppercase tracking-wide mb-sm">
            Adapters · {filtered.length}
          </h3>
          {filtered.length === 0 ? (
            <div className="text-center py-md text-on-surface-variant text-label-md">
              {t('extensions.datasources.noAdapters')}
            </div>
          ) : (
            <div className="grid grid-cols-1 md:grid-cols-2 gap-md">
              {filtered.map((entry) => {
                const slug = entrySlug(entry.id);
                const isInstalled = installedSlugs.has(slug);
                const isInstalling = installingSlug === entry.id;
                return (
                  <AdapterCard
                    key={entry.id}
                    entry={entry}
                    isInstalled={isInstalled}
                    isInstalling={isInstalling}
                    installForm={installForm}
                    busy={busySlug === entry.id}
                    feedback={feedback?.slug === entry.id ? feedback : null}
                    onFormChange={(key, value) =>
                      setInstallForm((prev) => ({ ...prev, [key]: value }))
                    }
                    onStartInstall={() => startInstall(entry)}
                    onCancelInstall={() => {
                      setInstallingSlug(null);
                      setFeedback(null);
                    }}
                    onSubmitInstall={() => submitInstall(entry)}
                  />
                );
              })}
            </div>
          )}
        </section>
      )}

      <section>
        <h3 className="text-label-lg font-bold text-on-surface-variant uppercase tracking-wide mb-sm">
          Installed · {installed.length}
        </h3>
        {installedLoading ? (
          <div className="text-center py-md text-on-surface-variant text-label-sm">{t('extensions.datasources.loadingInstalled')}</div>
        ) : installed.length === 0 ? (
          <div className="text-center py-md text-on-surface-variant text-label-sm">
            {t('extensions.datasources.noInstalled')}
          </div>
        ) : (
          <div className="border border-outline-variant/30 rounded-2xl overflow-hidden bg-surface-container-lowest/50">
            {installed.map((row, i) => (
              <div
                key={row.slug}
                className={`flex items-center gap-md px-md py-sm ${i === installed.length - 1 ? "" : "border-b border-outline-variant/15"}`}
              >
                <span className="material-symbols-outlined text-primary text-[20px]">database</span>
                <div className="flex-1 min-w-0">
                  <div className="font-bold text-label-md text-on-surface truncate">{row.name}</div>
                  <div className="text-label-xs text-on-surface-variant font-mono truncate">
                    {row.slug} · {row.kind}
                  </div>
                  <div className="text-label-xs text-on-surface-variant font-mono truncate">
                    {row.path}
                  </div>
                </div>
                <button
                  type="button"
                  onClick={() => handleUninstall(row.slug)}
                  disabled={busySlug === `uninstall:${row.slug}`}
                  className="px-sm py-xs rounded-lg bg-error-container/40 text-on-error-container text-label-xs font-bold hover:bg-error-container/70 disabled:opacity-50"
                >
                  {busySlug === `uninstall:${row.slug}` ? "…" : t('extensions.datasources.remove')}
                </button>
              </div>
            ))}
          </div>
        )}
      </section>
    </div>
  );
}

function AdapterCard({
  entry,
  isInstalled,
  isInstalling,
  installForm,
  busy,
  feedback,
  onFormChange,
  onStartInstall,
  onCancelInstall,
  onSubmitInstall,
}: {
  entry: DataSourceCatalogEntry;
  isInstalled: boolean;
  isInstalling: boolean;
  installForm: Record<string, string>;
  busy: boolean;
  feedback: { slug: string; msg: string; ok: boolean } | null;
  onFormChange: (key: string, value: string) => void;
  onStartInstall: () => void;
  onCancelInstall: () => void;
  onSubmitInstall: () => void;
}) {
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })

  const fields: DataSourceField[] = entry.metadata.fields ?? [];
  const icon = entry.metadata.kind === "email_imap" ? "mail" : "menu_book";
  return (
    <div className="border border-outline-variant/30 rounded-2xl p-md bg-surface-container-low/40 flex flex-col">
      <div className="flex items-start gap-sm mb-xs">
        <span className="material-symbols-outlined text-primary text-[24px] mt-[-2px]">{icon}</span>
        <div className="flex-1">
          <h4 className="font-bold text-label-md text-on-surface">{entry.name}</h4>
          <p className="text-label-sm text-on-surface-variant line-clamp-2">{entry.description}</p>
        </div>
        <span className="text-label-xs px-xs py-[1px] rounded-full font-bold bg-primary-container/50 text-on-primary-container">
          Verified
        </span>
      </div>

      {feedback && (
        <div className={`text-label-xs mb-xs ${feedback.ok ? "text-primary" : "text-error"}`}>
          {feedback.msg}
        </div>
      )}

      {!isInstalling ? (
        <div className="flex gap-xs mt-auto">
          {entry.homepage_url && (
            <a
              href={entry.homepage_url}
              target="_blank"
              rel="noreferrer"
              className="px-sm py-xs rounded-lg bg-surface-container-high text-on-surface text-label-xs font-bold hover:bg-surface-container-highest"
            >
              {t('extensions.datasources.view')}
            </a>
          )}
          <button
            type="button"
            onClick={onStartInstall}
            disabled={isInstalled}
            className="px-sm py-xs rounded-lg bg-primary text-on-primary text-label-xs font-bold hover:bg-primary/90 disabled:opacity-50 disabled:cursor-not-allowed"
          >
            {isInstalled ? t('extensions.datasources.installed') : t('extensions.datasources.configureInstall')}
          </button>
        </div>
      ) : (
        <form
          className="mt-auto space-y-sm"
          onSubmit={(e) => {
            e.preventDefault();
            onSubmitInstall();
          }}
        >
          {fields.map((field) => (
            <div key={field.key}>
              <label className="block text-label-xs font-bold text-on-surface-variant mb-[2px]">
                {field.label}
                {field.required ? <span className="text-error ml-[2px]">*</span> : null}
              </label>
              <input
                type={field.kind === "password" ? "password" : field.kind === "number" ? "text" : "text"}
                value={installForm[field.key] ?? ""}
                onChange={(e) => onFormChange(field.key, e.target.value)}
                placeholder={field.placeholder ?? ""}
                className="w-full px-sm py-xs rounded-lg bg-surface-container-lowest border border-outline-variant/50 text-label-sm font-mono focus:outline-none focus:border-primary"
              />
              {field.help && (
                <p className="text-label-xs text-on-surface-variant mt-[2px]">{field.help}</p>
              )}
            </div>
          ))}
          <div className="flex gap-xs pt-xs">
            <button
              type="submit"
              disabled={busy}
              className="px-sm py-xs rounded-lg bg-primary text-on-primary text-label-xs font-bold hover:bg-primary/90 disabled:opacity-50"
            >
              {busy ? t('extensions.datasources.saving') : t('extensions.datasources.save')}
            </button>
            <button
              type="button"
              onClick={onCancelInstall}
              disabled={busy}
              className="px-sm py-xs rounded-lg bg-surface-container-high text-on-surface text-label-xs font-bold hover:bg-surface-container-highest disabled:opacity-50"
            >
              {t('extensions.datasources.cancel')}
            </button>
          </div>
        </form>
      )}
    </div>
  );
}

/** Convert catalog id `native:data-source-obsidian-vault` → slug `obsidian-vault`. */
function entrySlug(id: string): string {
  const prefix = "native:data-source-";
  return id.startsWith(prefix) ? id.slice(prefix.length) : id;
}
