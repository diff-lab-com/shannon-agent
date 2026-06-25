import { useState, useEffect } from "react";
import { useIntl } from 'react-intl'
import {
  queryDataSource,
  listInstalledDataSources,
  type InstalledDataSource,
} from "@/lib/tauri-api";
import type { DataSourceResult, DataSourceItem } from "@/types";
import LoadingState from "@/components/ui/loading-state";

/**
 * Query panel for installed data sources.
 * Allows users to search across their personal data (Obsidian vaults, email, etc.)
 */
export default function DataSourcesQuery({ onSwitchToAdapters }: { onSwitchToAdapters?: () => void }) {
  const intl = useIntl()
  const t = (id: string, values?: Record<string, string | number>) => intl.formatMessage({ id }, values)

  const [installed, setInstalled] = useState<InstalledDataSource[]>([]);
  const [installedLoading, setInstalledLoading] = useState(true);
  const [selectedSlug, setSelectedSlug] = useState<string>("");
  const [query, setQuery] = useState("");
  const [results, setResults] = useState<DataSourceResult | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // Load installed data sources on mount
  useEffect(() => {
    listInstalledDataSources()
      .then(setInstalled)
      .finally(() => setInstalledLoading(false));
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

  if (installed.length === 0) {
    return (
      <div className="p-lg max-w-5xl mx-auto">
        <div className="text-center py-3xl text-on-surface-variant text-body-md">
          <span className="material-symbols-outlined text-[48px] text-outline mb-md">database_off</span>
          <p className="mb-md">{t('extensions.datasources.query.noDataSourcesInstalled')}</p>
          {onSwitchToAdapters && (
            <button
              type="button"
              onClick={onSwitchToAdapters}
              className="inline-flex items-center gap-xs px-md py-sm rounded-lg bg-primary text-on-primary text-label-md font-bold hover:bg-primary/90 cursor-pointer"
            >
              <span className="material-symbols-outlined text-[18px]">addon</span>
              {t('extensions.datasources.query.installCta')}
            </button>
          )}
        </div>
      </div>
    );
  }

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
            className="w-full px-sm py-sm rounded-lg bg-surface-container-lowest border border-outline-variant/50 text-label-md focus:outline-none focus:border-primary"
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
            className="w-full px-sm py-sm rounded-lg bg-surface-container-lowest border border-outline-variant/50 text-label-md focus:outline-none focus:border-primary"
          />
        </div>

        <button
          type="submit"
          disabled={!selectedSlug || !query || loading}
          className="px-md py-sm rounded-lg bg-primary text-on-primary text-label-md font-bold hover:bg-primary/90 disabled:opacity-50 disabled:cursor-not-allowed"
        >
          {loading ? t('extensions.datasources.query.searching') : t('extensions.datasources.query.searchButton')}
        </button>
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
            {(() => {
              const src = installed.find((s) => s.slug === selectedSlug)
              return src ? (
                <div className="text-label-sm text-on-surface-variant">{src.name}</div>
              ) : null
            })()}
          </div>

          {results.items.length === 0 ? (
            <div className="text-center py-md text-on-surface-variant text-label-md">
              {t('extensions.datasources.query.noResults')}
            </div>
          ) : (
            <div className="space-y-sm">
              {results.items.map((item, index) => (
                <ResultCard key={index} item={item} />
              ))}
            </div>
          )}
        </div>
      )}
    </div>
  );
}

function ResultCard({ item }: { item: DataSourceItem }) {
  const intl = useIntl()
  const t = (id: string, values?: Record<string, string | number>) => intl.formatMessage({ id }, values)

  const formatDate = (value: string | null | undefined) => {
    if (!value) return '-';
    const d = new Date(value);
    return isNaN(d.getTime()) ? '-' : d.toLocaleDateString();
  };

  return (
    <div className="border border-outline-variant/30 rounded-xl p-md bg-surface-container-low/50 hover:bg-surface-container-low transition-colors">
      <div className="flex items-start justify-between gap-sm mb-xs">
        <h4 className="font-bold text-label-md text-on-surface flex-1">
          {item.title}
        </h4>
        {item.url && (
          <a
            href={item.url}
            target="_blank"
            rel="noreferrer"
            className="text-label-xs px-sm py-xs rounded-lg bg-primary-container/20 text-on-primary-container font-bold hover:bg-primary-container/40 flex items-center gap-xs"
          >
            <span className="material-symbols-outlined text-[16px]">open_in_new</span>
            {t('extensions.datasources.query.openLink')}
          </a>
        )}
      </div>

      <p className="text-label-sm text-on-surface-variant line-clamp-3 mb-sm">
        {item.body}
      </p>

      <div className="flex items-center gap-md text-label-xs text-on-surface-variant">
        <div className="flex items-center gap-xs">
          <span className="material-symbols-outlined text-[16px]">category</span>
          <span>{item.kind}</span>
        </div>
        <div className="flex items-center gap-xs">
          <span className="material-symbols-outlined text-[16px]">schedule</span>
          <span>{formatDate(item.updated_at)}</span>
        </div>
      </div>
    </div>
  );
}
