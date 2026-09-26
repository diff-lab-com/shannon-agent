// PluginBundleBody — X5 trust-preview body for plugin-kind entries whose
// source is a git repo (marketplace plugin bundles).
//
// Before the user confirms, this body fetches `inspect_plugin_source` and
// renders the exact bundle checklist that will materialize into Shannon's
// extension homes: N skills, M agents, K commands, and the MCP server
// names the manifest declares. Install stays disabled until the preview
// has been read — an informed consent needs the list it consents to.
//
// SEC-1: the first install attempt runs with the default (no opt-in)
// consent. If the backend refuses an unverified manifest, the dialog
// flips `needsUnverifiedConsent` and this body surfaces an explicit
// "Install unverified anyway" action instead of silently retrying.

import { useEffect, useState } from 'react'
import { FormattedMessage, useIntl } from 'react-intl'
import * as api from '@/lib/tauri-api'
import type { PluginBundleSummary } from '@/types'
import { safeErrorMessage } from '@/lib/packageValidation'
import { Button } from '@/components/ui/button'

interface PluginBundleBodyProps {
  /** Git clone URL the inspect + install flow uses. */
  url: string
  installing: boolean
  needsUnverifiedConsent: boolean
  onInstall: (allowUnverified: boolean) => void
}

interface BundleRow {
  testid: string
  id: string
  names: string[]
}

function rowsFor(summary: PluginBundleSummary): BundleRow[] {
  return [
    {
      testid: 'bundle-skills',
      id: 'extensions.installDialog.bundle.skills',
      names: summary.skills,
    },
    {
      testid: 'bundle-agents',
      id: 'extensions.installDialog.bundle.agents',
      names: summary.agents,
    },
    {
      testid: 'bundle-commands',
      id: 'extensions.installDialog.bundle.commands',
      names: summary.commands,
    },
    {
      testid: 'bundle-mcp',
      id: 'extensions.installDialog.bundle.mcp',
      names: summary.mcp_servers,
    },
  ]
}

export function PluginBundleBody({
  url,
  installing,
  needsUnverifiedConsent,
  onInstall,
}: PluginBundleBodyProps) {
  const intl = useIntl()
  const [summary, setSummary] = useState<PluginBundleSummary | null>(null)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    let cancelled = false
    setSummary(null)
    setError(null)
    api
      .inspectPluginSource(url)
      .then((s) => {
        if (!cancelled) setSummary(s)
      })
      .catch((e) => {
        if (!cancelled) setError(safeErrorMessage(e, 'bundle preview failed'))
      })
    return () => {
      cancelled = true
    }
  }, [url])

  const rows = summary
    ? rowsFor(summary).filter((r) => r.names.length > 0)
    : []
  const bundleEmpty = summary !== null && rows.length === 0
  const blocked = error !== null || summary === null

  return (
    <div className="flex flex-col gap-md">
      <div className="flex items-center gap-sm">
        <span className="material-symbols-outlined text-[18px] text-on-surface-variant">
          workspaces
        </span>
        <span className="font-body-md font-mono text-on-surface truncate" title={url}>
          {url}
        </span>
      </div>

      <section
        data-testid="plugin-bundle-card"
        aria-label={intl.formatMessage({ id: 'extensions.installDialog.bundle.title' })}
        className="rounded-xl border border-outline-variant/30 bg-surface-container-low/60 p-md flex flex-col gap-xs"
      >
        <h5 className="text-label-md font-bold text-on-surface">
          <FormattedMessage id="extensions.installDialog.bundle.title" />
        </h5>
        {error !== null ? (
          <p data-testid="bundle-error" className="text-label-sm text-on-error-container bg-error-container/50 rounded-md px-sm py-xs">
            <FormattedMessage id="extensions.installDialog.bundle.loadError" />
            {' · '}
            <span className="text-on-surface-variant">{error}</span>
          </p>
        ) : summary === null ? (
          <p className="text-label-sm text-on-surface-variant">
            <FormattedMessage id="extensions.installDialog.bundle.inspecting" />
          </p>
        ) : (
          <>
            {rows.map((row) => (
              <div
                key={row.id}
                data-testid={row.testid}
                className="flex items-start gap-xs text-label-sm text-on-surface-variant"
              >
                <span className="material-symbols-outlined text-[14px] mt-[2px]" aria-hidden="true">
                  add_circle
                </span>
                <span>
                  {intl.formatMessage({ id: row.id }, {
                    count: row.names.length,
                    names: row.names.join(', '),
                  })}
                </span>
              </div>
            ))}
            {bundleEmpty && (
              <p data-testid="bundle-empty" className="text-label-sm text-on-surface-variant">
                <FormattedMessage id="extensions.installDialog.bundle.empty" />
              </p>
            )}
          </>
        )}
      </section>

      {needsUnverifiedConsent && (
        <p
          data-testid="unverified-warning"
          className="text-label-sm text-on-warning-container bg-warning-container/40 rounded-md px-sm py-xs"
        >
          <FormattedMessage id="extensions.installDialog.bundle.unverifiedBlocked" />
        </p>
      )}

      <div className="flex items-center gap-sm flex-wrap">
        <Button
          type="button"
          onClick={() => onInstall(false)}
          disabled={installing || blocked || needsUnverifiedConsent}
          className="px-md py-sm rounded-lg hover:bg-primary/90 disabled:opacity-60 cursor-pointer"
        >
          <span className="material-symbols-outlined icon-sm">
            {installing ? 'progress_activity' : 'download'}
          </span>
          {installing ? (
            <FormattedMessage id="extensions.installDialog.installing" />
          ) : (
            <FormattedMessage id="extensions.installDialog.install" />
          )}
        </Button>
        {needsUnverifiedConsent && (
          <Button
            type="button"
            variant="secondary"
            data-testid="install-unverified"
            onClick={() => onInstall(true)}
            disabled={installing}
            className="px-md py-sm rounded-lg disabled:opacity-60 cursor-pointer"
          >
            <span className="material-symbols-outlined icon-sm">gpp_maybe</span>
            <FormattedMessage id="extensions.installDialog.bundle.installAnyway" />
          </Button>
        )}
      </div>
    </div>
  )
}
