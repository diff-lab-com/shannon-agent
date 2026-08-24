// InstallDialog body for `mcp_registry` with a recognised package shape
// (npm/pip/docker). Previews the stdio command then installs via
// `installMcpStdio`. Disabled when `buildStdioSpec` couldn't validate the
// package (defence against a malicious registry entry).
//
// Extracted from InstallDialog.tsx (T3.1).

import { FormattedMessage } from 'react-intl'
import { Button } from '@/components/ui/button'
import { buildStdioSpec } from './types'

interface StdioBodyProps {
  pkgType: string | undefined
  pkgName: string | undefined
  installing: boolean
  onInstall: () => void
}

export function StdioBody({ pkgType, pkgName, installing, onInstall }: StdioBodyProps) {
  const spec = buildStdioSpec(pkgType, pkgName)

  return (
    <div className="flex flex-col gap-md">
      <label className="flex flex-col gap-xs">
        <span className="text-label-sm font-bold text-on-surface">
          <FormattedMessage id="extensions.installDialog.packageType" />
        </span>
        <span className="font-body-md text-on-surface">{pkgType ?? '—'}</span>
      </label>
      <label className="flex flex-col gap-xs">
        <span className="text-label-sm font-bold text-on-surface">
          <FormattedMessage id="extensions.installDialog.commandPreview" />
        </span>
        <code className="bg-surface-container-low border border-outline-variant/40 rounded-lg px-md py-sm font-body-md font-mono text-on-surface text-label-sm break-all">
          {spec ? [spec.command, ...spec.args].join(' ') : '—'}
        </code>
      </label>
      <Button
        type="button"
        onClick={onInstall}
        disabled={installing || !spec}
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
    </div>
  )
}