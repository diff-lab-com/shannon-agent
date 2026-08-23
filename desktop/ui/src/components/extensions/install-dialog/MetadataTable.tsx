// InstallDialog — renders `CatalogEntry.metadata` as a key/value table so
// the user sees ALL config the source provided, not just the fields the
// install branch knows about. Secret-shaped keys are redacted.
//
// Extracted from InstallDialog.tsx (T3.1).

import { FormattedMessage, useIntl } from 'react-intl'

const SECRET_KEY_PATTERNS = /token|secret|password|api[_-]?key|bearer|credential/i

function isSecretKey(key: string): boolean {
  return SECRET_KEY_PATTERNS.test(key)
}

function MetadataValue({ value }: { value: unknown }) {
  if (value === null || value === undefined) {
    return <span className="text-on-surface-variant">—</span>
  }
  if (typeof value === 'boolean') {
    return <span className="font-mono">{String(value)}</span>
  }
  if (typeof value === 'number') {
    return <span className="font-mono">{String(value)}</span>
  }
  if (typeof value === 'string') {
    // URL-detection: render long http(s) strings as links.
    if (value.length > 40 && /^https?:\/\//.test(value)) {
      return (
        <a
          href={value}
          target="_blank"
          rel="noopener noreferrer"
          className="text-primary hover:underline break-all"
        >
          {value}
        </a>
      )
    }
    return <span className="break-all">{value}</span>
  }
  if (Array.isArray(value)) {
    if (value.length === 0) return <span className="text-on-surface-variant">[]</span>
    return (
      <div className="flex flex-wrap gap-xs">
        {value.map((v, i) => (
          <span
            key={i}
            className="px-xs py-[1px] rounded bg-surface-container-high text-label-xs font-mono"
          >
            {typeof v === 'string' ? v : JSON.stringify(v)}
          </span>
        ))}
      </div>
    )
  }
  if (typeof value === 'object') {
    return (
      <pre className="bg-surface-container-low p-sm rounded text-label-xs font-mono overflow-x-auto">
        {JSON.stringify(value, null, 2)}
      </pre>
    )
  }
  return <span className="text-on-surface-variant">{String(value)}</span>
}

export function MetadataTable({ metadata }: { metadata: Record<string, unknown> | undefined | null }) {
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })

  if (!metadata || Object.keys(metadata).length === 0) {
    return (
      <div className="border-t border-outline-variant/20 pt-md mt-md">
        <h4 className="text-label-sm font-bold text-on-surface mb-sm">
          <FormattedMessage id="extensions.installDialog.metadata.title" />
        </h4>
        <p className="text-label-sm text-on-surface-variant">
          <FormattedMessage id="extensions.installDialog.metadata.empty" />
        </p>
      </div>
    )
  }

  const entries = Object.entries(metadata).sort(([a], [b]) => a.localeCompare(b))

  return (
    <div className="border-t border-outline-variant/20 pt-md mt-md">
      <h4 className="text-label-sm font-bold text-on-surface mb-sm">
        <FormattedMessage id="extensions.installDialog.metadata.title" />
      </h4>
      <dl className="grid grid-cols-[auto_1fr] gap-x-md gap-y-xs">
        {entries.map(([key, value]) => {
          const secret = isSecretKey(key)
          return (
            <div key={key} className="contents">
              <dt className="text-label-sm font-bold text-on-surface-variant font-mono whitespace-nowrap">
                {key}
              </dt>
              <dd className="text-label-sm text-on-surface min-w-0">
                {secret && typeof value === 'string' && value.length > 0 ? (
                  <span
                    className="font-mono text-on-surface-variant select-none"
                    title={t('extensions.installDialog.metadata.redacted')}
                  >
                    {t('extensions.installDialog.metadata.redacted')}
                  </span>
                ) : (
                  <MetadataValue value={value} />
                )}
              </dd>
            </div>
          )
        })}
      </dl>
    </div>
  )
}