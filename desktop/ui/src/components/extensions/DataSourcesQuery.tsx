import { useState, useEffect } from "react";
import { useIntl } from 'react-intl'
import { useNavigate } from "react-router-dom";
import { toast } from 'sonner'
import {
  queryDataSource,
  listInstalledDataSources,
  type InstalledDataSource,
} from "@/lib/tauri-api";
import type { DataSourceResult, DataSourceItem } from "@/types";
// Office Wave 2 B3 — bridge into the chat composer draft. The bridge module
// (window CustomEvent 'shannon:composer-draft') lands on this same branch.
import { pushComposerDraft } from "@/lib/composerBridge";
import LoadingState from "@/components/ui/loading-state";
import ErrorState from "@/components/ui/error-state";
import { Button } from "@/components/ui/button";

/// Office Wave 2 B3 — an excerpt longer than this is truncated before it
/// lands in the composer draft, so one huge note can't flood the context.
const MAX_EXCERPT_CHARS = 2000;

/**
 * Query panel for installed data sources.
 * Allows users to search across their personal data (Obsidian vaults, email, etc.)
 */
export default function DataSourcesQuery({ onSwitchToAdapters }: { onSwitchToAdapters?: () => void }) {
  const intl = useIntl()
  const t = (id: string, values?: Record<string, string | number>) => intl.formatMessage({ id }, values)

  const [installed, setInstalled] = useState<InstalledDataSource[]>([]);
  const [installedLoading, setInstalledLoading] = useState(true);
  // B3 P1-17: a failed read must not render as "no data sources installed".
  const [installedError, setInstalledError] = useState<string | null>(null);
  const [selectedSlug, setSelectedSlug] = useState<string>("");
  const [query, setQuery] = useState("");
  const [results, setResults] = useState<DataSourceResult | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const refreshInstalled = () => {
    listInstalledDataSources()
      .then((rows) => {
        setInstalled(rows);
        setInstalledError(null);
      })
      .catch((err) => {
        setInstalledError(err instanceof Error ? err.message : String(err));
      })
      .finally(() => setInstalledLoading(false));
  };

  // Load installed data sources on mount
  useEffect(() => {
    refreshInstalled();
  }, []);

  async function handleSearch(e: React.FormEvent) {
    e.preventDefault();
    if (!selectedSlug.trim() || !query.trim()) return;

    setLoading(true);
    setError(null);
    setResults(null);

    try {
      const result = await queryDataSource(selectedSlug, query);
      setResults(result);
    } catch (err) {
      setError(String(err));
    } finally {
      setLoading(false);
    }
  }

  if (installedLoading) {
    return (
      <div className="p-lg max-w-5xl mx-auto">
        <LoadingState size="sm" label={t('extensions.datasources.loadingInstalled')} />
      </div>
    );
  }

  if (installedError) {
    return (
      <div className="p-lg max-w-5xl mx-auto">
        <ErrorState
          icon="database"
          title={t('extensions.datasources.installedLoadFailed')}
          description={installedError}
          action={{ label: t('common.retry'), onClick: refreshInstalled }}
        />
      </div>
    );
  }

  if (installed.length === 0) {
    return (
      <div className="p-lg max-w-5xl mx-auto">
        <div className="text-center py-3xl text-on-surface-variant text-body-md">
          <span className="material-symbols-outlined icon-2xl text-outline mb-md">database_off</span>
          <p className="mb-md">{t('extensions.datasources.query.noDataSourcesInstalled')}</p>
          {onSwitchToAdapters && (
            <Button
              type="button"
              onClick={onSwitchToAdapters}
              className="cursor-pointer"
            >
              <span className="material-symbols-outlined icon-md">addon</span>
              {t('extensions.datasources.query.installCta')}
            </Button>
          )}
        </div>
      </div>
    );
  }

  // The installed source behind the current query — used for the header label
  // and as the Add-to-chat fallbacks (name when the item has no title, path
  // when the item has no url).
  const activeSource = results ? installed.find((s) => s.slug === selectedSlug) ?? null : null;

  return (
    <div className="p-lg max-w-5xl mx-auto space-y-xl">
      <header>
        <h2 className="text-headline-md font-headline-md text-on-surface mb-xs">
          {t('extensions.datasources.query.title')}
        </h2>
        <p className="text-body-md text-on-surface-variant">
          {t('extensions.datasources.query.subtitle')}
        </p>
      </header>

      <form onSubmit={handleSearch} className="space-y-md">
        <div>
          <label htmlFor="dataSourceSelect" className="block text-label-sm font-bold text-on-surface-variant mb-xs">
            {t('extensions.datasources.query.selectSource')}
          </label>
          <select
            id="dataSourceSelect"
            value={selectedSlug}
            onChange={(e) => setSelectedSlug(e.target.value)}
            className="w-full px-sm py-sm rounded-lg bg-surface-container-lowest border border-outline-variant/50 text-label-md focus-visible:border-primary focus-visible:ring-2 focus-visible:ring-primary/30"
          >
            <option value="">{t('extensions.datasources.query.selectSourcePlaceholder')}</option>
            {installed.map((source) => (
              <option key={source.slug} value={source.slug}>
                {source.name} ({source.slug})
              </option>
            ))}
          </select>
        </div>

        <div>
          <label htmlFor="queryInput" className="block text-label-sm font-bold text-on-surface-variant mb-xs">
            {t('extensions.datasources.query.queryPlaceholder')}
          </label>
          <input
            id="queryInput"
            type="text"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder={t('extensions.datasources.query.queryPlaceholder')}
            className="w-full px-sm py-sm rounded-lg bg-surface-container-lowest border border-outline-variant/50 text-label-md focus-visible:border-primary focus-visible:ring-2 focus-visible:ring-primary/30"
          />
        </div>

        <Button
          type="submit"
          disabled={!selectedSlug || !query || loading}
          className="disabled:cursor-not-allowed"
        >
          {loading ? t('extensions.datasources.query.searching') : t('extensions.datasources.query.searchButton')}
        </Button>
      </form>

      {error && (
        <div className="p-md rounded-lg bg-error-container/20 border border-error-container/50">
          <div className="flex items-start gap-sm">
            <span className="material-symbols-outlined text-error">error</span>
            <div>
              <div className="font-bold text-label-md text-on-error-container">
                {t('extensions.datasources.query.errorTitle')}
              </div>
              <div className="text-label-sm text-on-error-container mt-xs">{error}</div>
            </div>
          </div>
        </div>
      )}

      {results && (
        <div className="space-y-md">
          <div className="flex items-center justify-between">
            <h3 className="text-label-lg font-bold text-on-surface">
              {t('extensions.datasources.query.resultsCount', { count: results.total })}
            </h3>
            {activeSource ? (
              <div className="text-label-sm text-on-surface-variant">{activeSource.name}</div>
            ) : null}
          </div>

          {results.items.length === 0 ? (
            <div className="text-center py-md text-on-surface-variant text-label-md">
              {t('office.sources.noResults')}
            </div>
          ) : (
            <div className="space-y-sm">
              {results.items.map((item, index) => (
                <ResultCard
                  key={index}
                  item={item}
                  sourceName={activeSource?.name ?? selectedSlug}
                  sourcePath={activeSource?.path ?? ''}
                />
              ))}
            </div>
          )}
        </div>
      )}
    </div>
  );
}

function ResultCard({
  item,
  sourceName,
  sourcePath,
}: {
  item: DataSourceItem;
  sourceName: string;
  sourcePath: string;
}) {
  const intl = useIntl()
  const navigate = useNavigate()
  const t = (id: string, values?: Record<string, string | number>) => intl.formatMessage({ id }, values)

  const formatDate = (value: string | null | undefined) => {
    if (!value) return '-';
    const d = new Date(value);
    return isNaN(d.getTime()) ? '-' : d.toLocaleDateString();
  };

  // Office Wave 2 B3 — hand this result to the composer as a source-attributed
  // context block:
  //   [Source: <title|name>] (<url|path>)
  //   <body excerpt, capped at MAX_EXCERPT_CHARS>
  // G5 P0-6: the composer only exists on /chat. Pushing from
  // /extensions/datasources used to fire the draft event with nobody
  // listening — silent loss behind a success toast. The bridge now parks the
  // draft in its pending queue when no composer is mounted, and navigating
  // to /chat mounts ChatInput, which flushes the queue on subscribe.
  const handleAddToChat = () => {
    const label = item.title || sourceName;
    const location = item.url || sourcePath;
    const excerpt = (item.body ?? '').slice(0, MAX_EXCERPT_CHARS);
    pushComposerDraft(`[Source: ${label}] (${location})\n${excerpt}`);
    navigate('/chat');
    toast.success(t('office.sources.added'));
  };

  return (
    <div className="border border-outline-variant/30 rounded-xl p-md bg-surface-container-low/50 hover:bg-surface-container-low transition-colors">
      <div className="flex items-start justify-between gap-sm mb-xs">
        <h4 className="font-bold text-label-md text-on-surface flex-1">
          {item.title}
        </h4>
        <div className="flex items-center gap-xs shrink-0">
          <button
            type="button"
            onClick={handleAddToChat}
            className="text-label-xs px-sm py-xs rounded-lg bg-secondary-container/40 text-on-secondary-container font-bold hover:bg-secondary-container/70 flex items-center gap-xs cursor-pointer"
          >
            <span className="material-symbols-outlined icon-sm">chat_add_on</span>
            {t('office.sources.addToChat')}
          </button>
          {item.url && (
            <a
              href={item.url}
              target="_blank"
              rel="noreferrer"
              className="text-label-xs px-sm py-xs rounded-lg bg-primary-container/20 text-on-primary-container font-bold hover:bg-primary-container/40 flex items-center gap-xs"
            >
              <span className="material-symbols-outlined icon-sm">open_in_new</span>
              {t('extensions.datasources.query.openLink')}
            </a>
          )}
        </div>
      </div>

      <p className="text-label-sm text-on-surface-variant line-clamp-3 mb-sm">
        {item.body}
      </p>

      <div className="flex items-center gap-md text-label-xs text-on-surface-variant">
        <div className="flex items-center gap-xs">
          <span className="material-symbols-outlined icon-sm">category</span>
          <span>{item.kind}</span>
        </div>
        <div className="flex items-center gap-xs">
          <span className="material-symbols-outlined icon-sm">schedule</span>
          <span>{formatDate(item.updated_at)}</span>
        </div>
      </div>
    </div>
  );
}
