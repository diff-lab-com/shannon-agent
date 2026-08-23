// InstallDialog — modal that renders the right install form based on a
// CatalogEntry's source type. Orchestrator only (T3.1).
//
// The dialog reads `entry.source` (CatalogSource discriminated union) and
// `entry.metadata` (untyped record from the Rust side) to pick one of four
// bodies:
//   1. featured_vendor + metadata.transport=oauth_remote → OAuthBody
//      (vendor info + a placeholder "Connect" button).
//   2. git_hub_repo → GitHubBody — install via installSkillFromRepo /
//      installAgentFromRepo with a busy spinner and success/error toast.
//   3. mcp_registry + metadata.package → StdioBody — preview the stdio
//      command and install via installMcpStdio.
//   4. native / fallback → FallbackBody — tell the user to use the
//      dedicated tab, with a button that navigates via KIND_ROUTE.
//
// On a successful install the dialog dispatches `shannon:extension-installed`
// (same contract Plugins.tsx used before) so the Extensions shell can refresh.

import { useEffect, useState } from 'react'
import { useIntl } from 'react-intl'
import { useNavigate } from 'react-router-dom'
import { toast } from 'sonner'
import * as api from '@/lib/tauri-api'
import { safeErrorMessage } from '@/lib/packageValidation'
import { Modal, ModalBody } from '@/components/ui/modal'
import type { CatalogEntry } from '@/types'
import { FallbackBody } from './install-dialog/FallbackBody'
import { GitHubBody } from './install-dialog/GitHubBody'
import { MetadataTable } from './install-dialog/MetadataTable'
import { OAuthBody } from './install-dialog/OAuthBody'
import { StdioBody } from './install-dialog/StdioBody'
import { KIND_ROUTE, buildStdioSpec, readMeta } from './install-dialog/types'

export interface InstallDialogProps {
  entry: CatalogEntry | null
  open: boolean
  onClose: () => void
  onInstalled: () => void
}

export default function InstallDialog({
  entry,
  open,
  onClose,
  onInstalled,
}: InstallDialogProps) {
  const intl = useIntl()
  const navigate = useNavigate()
  const t = (id: string, values?: Record<string, string | number>) =>
    intl.formatMessage({ id }, values)

  const [installing, setInstalling] = useState(false)

  // Reset local state each time the dialog opens.
  useEffect(() => {
    if (open) setInstalling(false)
  }, [open])

  // Compute render-time data only when we have an entry. Modal also
  // returns null when !open, so the inner render never runs without it.
  if (!entry) return null
  const meta = readMeta(entry)

  const dispatchInstalled = () => {
    window.dispatchEvent(
      new CustomEvent('shannon:extension-installed', {
        detail: { kind: entry.kind, name: entry.name },
      }),
    )
    onInstalled()
  }

  const handleGitHubInstall = async () => {
    if (entry.source.type !== 'git_hub_repo') return
    const ref_ = entry.source.ref_ || 'main'
    setInstalling(true)
    try {
      let result: api.InstallResult
      if (entry.kind === 'skill') {
        result = await api.installSkillFromRepo(entry.name, entry.source.repo, ref_)
      } else if (entry.kind === 'agent') {
        result = await api.installAgentFromRepo(entry.name, entry.source.repo, ref_)
      } else {
        // Other GitHub kinds aren't backed by a dedicated installer yet.
        toast.error(t('extensions.plugins.installError', { error: entry.kind }))
        return
      }
      toast.success(
        intl.formatMessage(
          { id: 'extensions.plugins.installSuccess' },
          { name: result.name },
        ),
        {
          description: result.install_path ?? undefined,
        },
      )
      dispatchInstalled()
      onClose()
    } catch (e) {
      console.error('GitHub install error:', e)
      toast.error(
        intl.formatMessage(
          { id: 'extensions.plugins.installError' },
          { error: safeErrorMessage(e, 'install failed') },
        ),
      )
    } finally {
      setInstalling(false)
    }
  }

  const handleStdioInstall = async () => {
    const spec = buildStdioSpec(meta.package?.type, meta.package?.name)
    if (!spec) return
    setInstalling(true)
    try {
      const result = await api.installMcpStdio({
        server_name: entry.name,
        command: spec.command,
        args: spec.args,
        env: [],
      })
      toast.success(
        intl.formatMessage(
          { id: 'extensions.plugins.installSuccess' },
          { name: result.name },
        ),
      )
      dispatchInstalled()
      onClose()
    } catch (e) {
      console.error('MCP stdio install error:', e)
      toast.error(
        intl.formatMessage(
          { id: 'extensions.plugins.installError' },
          { error: safeErrorMessage(e, 'install failed') },
        ),
      )
    } finally {
      setInstalling(false)
    }
  }

  const handleOAuthConnect = () => {
    // Backend OAuth installer is not wired yet — be explicit so the user
    // isn't left wondering why nothing happened.
    toast.info(t('extensions.installDialog.oauthComingSoon'))
  }

  const handleOpenTab = () => {
    const route = KIND_ROUTE[entry.kind]
    if (route) {
      navigate(route)
      onClose()
    }
  }

  // ---- Body selection ---------------------------------------------------

  const renderBody = () => {
    switch (entry.source.type) {
      case 'featured_vendor': {
        if (meta.transport !== 'oauth_remote') {
          // Featured vendor without OAuth metadata — fall through to the
          // "manual configuration" branch.
          break
        }
        return (
          <OAuthBody
            vendor={meta.vendor ?? entry.author ?? entry.name}
            endpoint={meta.endpoint}
            scopes={meta.scopes ?? []}
            onConnect={handleOAuthConnect}
          />
        )
      }
      case 'git_hub_repo': {
        const ref_ = entry.source.ref_ || 'main'
        return (
          <GitHubBody
            repo={entry.source.repo}
            ref_={ref_}
            installing={installing}
            onInstall={handleGitHubInstall}
          />
        )
      }
      case 'mcp_registry': {
        return (
          <StdioBody
            pkgType={meta.package?.type}
            pkgName={meta.package?.name}
            installing={installing}
            onInstall={handleStdioInstall}
          />
        )
      }
      case 'custom':
      case 'native':
      default:
        break
    }

    // Fallback: manual configuration routed to the dedicated tab.
    return <FallbackBody kind={entry.kind} onOpenTab={handleOpenTab} />
  }

  return (
    <Modal
      open={open && !!entry}
      onClose={onClose}
      size="lg"
      title={t('extensions.installDialog.title', { name: entry.name })}
      closeLabel={t('extensions.installDialog.closeAria')}
      busy={installing}
      className="max-h-[90vh] overflow-y-auto"
    >
      <ModalBody className="flex flex-col gap-md">
        {entry.description ? (
          <p className="text-label-sm text-on-surface-variant">
            {entry.description}
          </p>
        ) : null}

        {renderBody()}

        <MetadataTable metadata={entry.metadata} />
      </ModalBody>
    </Modal>
  )
}