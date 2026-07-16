import { useIntl } from 'react-intl'
import type { SkillCatalogEntry } from '@/lib/tauri-api'

interface SkillDetailDrawerProps {
  entry: SkillCatalogEntry | null
  installed: boolean
  busy: boolean
  onClose: () => void
  onInstall: () => void
}

function formatLastUpdated(ts: string | null): string {
  if (!ts) return ''
  const d = new Date(ts)
  if (Number.isNaN(d.getTime())) return ''
  return d.toLocaleDateString()
}

function describeSource(entry: SkillCatalogEntry): string {
  switch (entry.source.type) {
    case 'native':
      return 'Built-in'
    case 'mcp_registry':
      return `Registry · ${entry.source.publisher}`
    case 'featured_vendor':
      return 'Featured vendor'
    case 'git_hub_repo': {
      const refPart = entry.source.ref_ ? `@ ${entry.source.ref_}` : '@ main'
      return `${entry.source.repo} ${refPart}`
    }
    case 'custom':
      return entry.source.url
  }
}

const TRUST_LABELS: Record<SkillCatalogEntry['trust'], { cls: string }> = {
  verified: { cls: 'bg-primary-container/50 text-on-primary-container' },
  official: { cls: 'bg-secondary-container/50 text-on-secondary-container' },
  community: { cls: 'bg-tertiary-container/50 text-on-tertiary-container' },
  unknown: { cls: 'bg-surface-container-highest text-on-surface-variant' },
}

export default function SkillDetailDrawer({
  entry,
  installed,
  busy,
  onClose,
  onInstall,
}: SkillDetailDrawerProps) {
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })
  const na = t('extensions.skills.drawer.notAvailable')

  if (!entry) return null
  const trust = TRUST_LABELS[entry.trust]
  const trustTextKey = `extensions.skills.trust.${entry.trust}`
  const lastUpdated = formatLastUpdated(entry.last_updated)

  return (
    <div
      className="fixed inset-0 z-50 flex justify-end"
      onClick={onClose}
      onKeyDown={e => { if (e.key === 'Escape') onClose() }}
      role="dialog"
      aria-modal="true"
      aria-label={intl.formatMessage({ id: 'extensions.skills.drawer.ariaLabel' }, { name: entry.name })}
    >
      <div className="bg-black/20 absolute inset-0" />
      <div
        className="relative w-[440px] max-w-full bg-surface-container-lowest shadow-2xl border-l border-outline-variant/20 p-xl overflow-y-auto"
        onClick={e => e.stopPropagation()}
      >
        <div className="flex items-start justify-between mb-lg gap-sm">
          <div className="min-w-0">
            <h3 className="font-headline-md text-on-surface font-bold truncate">{entry.name}</h3>
            <span className={`inline-block mt-xs text-label-xs px-sm py-[2px] rounded-full font-bold ${trust.cls}`}>
              {intl.formatMessage({ id: trustTextKey })}
            </span>
          </div>
          <button
            type="button"
            onClick={onClose}
            aria-label={t('extensions.skills.drawer.close')}
            className="p-xs rounded-full text-on-surface-variant hover:bg-surface-container-high hover:text-on-surface shrink-0"
          >
            <span className="material-symbols-outlined text-[20px]">close</span>
          </button>
        </div>

        <dl className="space-y-md">
          <div>
            <dt className="text-label-sm text-on-surface-variant mb-xs">
              {t('extensions.skills.drawer.description')}
            </dt>
            <dd className="font-body-md text-on-surface">{entry.description}</dd>
          </div>

          <div className="grid grid-cols-2 gap-md">
            <div>
              <dt className="text-label-xs text-on-surface-variant uppercase tracking-wide">
                {t('extensions.skills.drawer.author')}
              </dt>
              <dd className="font-label-md text-on-surface font-mono break-all">
                {entry.author ?? na}
              </dd>
            </div>
            <div>
              <dt className="text-label-xs text-on-surface-variant uppercase tracking-wide">
                {t('extensions.skills.drawer.version')}
              </dt>
              <dd className="font-label-md text-on-surface font-mono">
                {entry.version ?? na}
              </dd>
            </div>
            <div>
              <dt className="text-label-xs text-on-surface-variant uppercase tracking-wide">
                {t('extensions.skills.drawer.license')}
              </dt>
              <dd className="font-label-md text-on-surface font-mono">
                {entry.license ?? na}
              </dd>
            </div>
            <div>
              <dt className="text-label-xs text-on-surface-variant uppercase tracking-wide">
                {t('extensions.skills.drawer.stars')}
              </dt>
              <dd className="font-label-md text-on-surface flex items-center gap-xs">
                {entry.stars != null ? (
                  <>
                    <span className="material-symbols-outlined text-[14px]">star</span>
                    {entry.stars}
                  </>
                ) : na}
              </dd>
            </div>
            <div className="col-span-2">
              <dt className="text-label-xs text-on-surface-variant uppercase tracking-wide">
                {t('extensions.skills.drawer.source')}
              </dt>
              <dd className="font-label-md text-on-surface font-mono break-all">
                {describeSource(entry)}
              </dd>
            </div>
            {lastUpdated && (
              <div className="col-span-2">
                <dt className="text-label-xs text-on-surface-variant uppercase tracking-wide">
                  {t('extensions.skills.drawer.lastUpdated')}
                </dt>
                <dd className="font-label-md text-on-surface">{lastUpdated}</dd>
              </div>
            )}
            {entry.tags.length > 0 && (
              <div className="col-span-2">
                <dt className="text-label-xs text-on-surface-variant uppercase tracking-wide mb-xs">
                  {t('extensions.skills.drawer.tags')}
                </dt>
                <dd className="flex flex-wrap gap-xs">
                  {entry.tags.map(tag => (
                    <span
                      key={tag}
                      className="text-label-xs px-sm py-[2px] rounded-full bg-surface-container-high text-on-surface-variant"
                    >
                      {tag}
                    </span>
                  ))}
                </dd>
              </div>
            )}
            {entry.homepage_url && (
              <div className="col-span-2">
                <dt className="text-label-xs text-on-surface-variant uppercase tracking-wide mb-xs">
                  {t('extensions.skills.drawer.homepage')}
                </dt>
                <dd>
                  <a
                    href={entry.homepage_url}
                    target="_blank"
                    rel="noreferrer"
                    className="text-label-md text-primary hover:underline break-all inline-flex items-center gap-xs"
                  >
                    <span className="material-symbols-outlined text-[14px]">open_in_new</span>
                    {entry.homepage_url}
                  </a>
                </dd>
              </div>
            )}
          </div>
        </dl>

        <div className="mt-xl flex gap-sm">
          <button
            type="button"
            onClick={onInstall}
            disabled={busy || installed}
            className="flex-1 px-md py-sm rounded-lg bg-primary text-on-primary text-label-md font-bold hover:bg-primary/90 disabled:opacity-50 disabled:cursor-not-allowed"
          >
            {busy ? '…' : installed ? t('extensions.skills.installedBtn') : t('extensions.skills.installBtn')}
          </button>
        </div>
      </div>
    </div>
  )
}
