// TrustCard — X2 安装时信任卡 ("will be enabled" panel).
//
// Shown inside the InstallDialog so the user sees WHAT they are about to
// enable BEFORE confirming. Everything rendered here is derived from data the
// dialog already has (CatalogEntry.source / .metadata / .description) —
// fields the manifest doesn't model (MCP tool lists, runtime filesystem /
// network permissions) are NOT invented; the after-install footer tells the
// user where those become visible instead.
//
// Derivations, per field:
//   - Source (always available): registry publisher / GitHub repo / featured
//     vendor / custom URL / local config.
//   - "Runs a local command": only when `buildStdioSpec` can validate the
//     registry package (npm/pip/docker) — same command the StdioBody
//     previews. Docker additionally implies container isolation.
//   - "Fetches from the network": any remote source (registry, GitHub,
//     vendor, custom URL) — the install itself downloads content.
//   - OAuth scopes: featured_vendor + transport=oauth_remote metadata only.
//   - Skill trigger description: CatalogEntry.description for kind=skill.

import { FormattedMessage, useIntl } from 'react-intl'
import type { CatalogEntry } from '@/types'
import { buildStdioSpec, readMeta } from './types'

export function TrustCard({ entry }: { entry: CatalogEntry }) {
  const intl = useIntl()
  const t = (id: string, values?: Record<string, string | number>) =>
    intl.formatMessage({ id }, values)
  const meta = readMeta(entry)

  // Command execution is only claimed when the package shape validates —
  // never from free-form metadata.
  const stdioSpec = buildStdioSpec(meta.package?.type, meta.package?.name)
  const isContainer = meta.package?.type === 'docker'
  const isRemoteSource =
    entry.source.type === 'mcp_registry' ||
    entry.source.type === 'git_hub_repo' ||
    entry.source.type === 'featured_vendor' ||
    entry.source.type === 'custom'
  const scopes = meta.scopes ?? []

  return (
    <section
      aria-label={t('extensions.installDialog.trust.title')}
      data-testid="install-trust-card"
      className="rounded-xl border border-outline-variant/30 bg-surface-container-low/60 p-md flex flex-col gap-sm"
    >
      <h4 className="flex items-center gap-xs text-label-md font-bold text-on-surface">
        <span className="material-symbols-outlined icon-sm text-primary" aria-hidden="true">
          verified_user
        </span>
        <FormattedMessage id="extensions.installDialog.trust.title" />
      </h4>

      {/* Source — derivable for every entry, so always shown. */}
      <div className="flex items-start gap-sm text-label-sm text-on-surface-variant">
        <span className="material-symbols-outlined text-[14px] mt-[2px]" aria-hidden="true">
          source
        </span>
        <div className="min-w-0">
          <FormattedMessage id="extensions.installDialog.trust.sourceLabel" />
          {' · '}
          {entry.source.type === 'mcp_registry' && (
            <span>
              {t('extensions.installDialog.trust.source.registry', {
                publisher: entry.source.publisher,
              })}
            </span>
          )}
          {entry.source.type === 'featured_vendor' && (
            <span>
              {t('extensions.installDialog.trust.source.vendor', {
                vendor: meta.vendor ?? entry.author ?? entry.name,
              })}
            </span>
          )}
          {entry.source.type === 'git_hub_repo' && (
            <span>
              {t('extensions.installDialog.trust.source.github')}{' '}
              <code className="font-mono text-on-surface">{entry.source.repo}</code>
            </span>
          )}
          {entry.source.type === 'custom' && (
            <span>{t('extensions.installDialog.trust.source.custom')}</span>
          )}
          {entry.source.type === 'native' && (
            <span>{t('extensions.installDialog.trust.source.native')}</span>
          )}
        </div>
      </div>

      {/* Skills: the trigger description is what gets enabled. */}
      {entry.kind === 'skill' && entry.description ? (
        <div className="flex items-start gap-sm text-label-sm text-on-surface-variant">
          <span className="material-symbols-outlined text-[14px] mt-[2px]" aria-hidden="true">
            bolt
          </span>
          <div className="min-w-0">
            <span className="font-bold text-on-surface">
              {t('extensions.installDialog.trust.skillDescription')}
            </span>
            {' · '}
            <span>{entry.description}</span>
          </div>
        </div>
      ) : null}

      {/* Derived capability chips — only what the manifest actually implies. */}
      {(stdioSpec || isContainer || isRemoteSource || scopes.length > 0) && (
        <div className="flex flex-wrap gap-xs">
          {stdioSpec && (
            <span
              data-testid="trust-capability-command"
              className="inline-flex items-center gap-xs px-xs py-[2px] rounded-full bg-warning-container/40 text-on-warning-container text-label-xs font-bold"
              title={[stdioSpec.command, ...stdioSpec.args].join(' ')}
            >
              <span className="material-symbols-outlined text-[12px]" aria-hidden="true">
                terminal
              </span>
              {t('extensions.installDialog.trust.capability.command')}
            </span>
          )}
          {isContainer && (
            <span className="inline-flex items-center gap-xs px-xs py-[2px] rounded-full bg-surface-container-high text-on-surface-variant text-label-xs font-bold">
              <span className="material-symbols-outlined text-[12px]" aria-hidden="true">
                deployable_code
              </span>
              {t('extensions.installDialog.trust.capability.container')}
            </span>
          )}
          {isRemoteSource && (
            <span className="inline-flex items-center gap-xs px-xs py-[2px] rounded-full bg-surface-container-high text-on-surface-variant text-label-xs font-bold">
              <span className="material-symbols-outlined text-[12px]" aria-hidden="true">
                cloud_download
              </span>
              {t('extensions.installDialog.trust.capability.fetch')}
            </span>
          )}
          {scopes.length > 0 && (
            <span className="inline-flex items-center gap-xs px-xs py-[2px] rounded-full bg-surface-container-high text-on-surface-variant text-label-xs font-bold">
              <span className="material-symbols-outlined text-[12px]" aria-hidden="true">
                key
              </span>
              {t('extensions.installDialog.trust.capability.scopes')}
            </span>
          )}
        </div>
      )}

      {/* Fallback for fields the catalog manifest doesn't model (MCP tool
          lists, runtime permissions): tell the user where they appear. */}
      <p className="text-label-xs text-on-surface-variant/80 flex items-center gap-xs">
        <span className="material-symbols-outlined text-[12px]" aria-hidden="true">
          info
        </span>
        <FormattedMessage id="extensions.installDialog.trust.afterInstall" />
      </p>
    </section>
  )
}
