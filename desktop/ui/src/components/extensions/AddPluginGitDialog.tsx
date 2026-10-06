// AddPluginGitDialog — X6 「添加插件 · 从 Git URL」 dialog.
//
// The Plugins page's add-menu entry point for remote plugin bundles. The
// user types a clone URL; the install clones the repo (depth 1) and
// materializes its bundle (skills / agents / commands / MCP servers).
//
// SEC-1 consent follows the InstallDialog pattern exactly (consistency was
// the task's binding choice): the first attempt runs with the default
// consent (`allow_unverified=false`). When the backend refuses an
// unverified (permissions-less) manifest the dialog flips
// `needsUnverifiedConsent` and surfaces an explicit "Install unverified
// anyway" opt-in instead of silently retrying — the backend refuses unless
// consented, and the refusal is what arms the opt-in.

import { useEffect, useState } from 'react'
import { FormattedMessage, useIntl } from 'react-intl'
import { toast } from 'sonner'
import * as api from '@/lib/tauri-api'
import { safeErrorMessage } from '@/lib/packageValidation'
import { Modal, ModalBody, ModalFooter } from '@/components/ui/modal'
import { Input } from '@/components/ui/input'
import { Button } from '@/components/ui/button'

export interface AddPluginGitDialogProps {
  open: boolean
  onClose: () => void
  /** Called after a successful install (already toasted). */
  onInstalled: (result: api.PluginInstallResult) => void
}

export default function AddPluginGitDialog({
  open,
  onClose,
  onInstalled,
}: AddPluginGitDialogProps) {
  const intl = useIntl()
  const t = (id: string, values?: Record<string, string | number>) =>
    intl.formatMessage({ id }, values)

  const [url, setUrl] = useState('')
  const [installing, setInstalling] = useState(false)
  // SEC-1: flipped when the backend refuses a permissions-less remote
  // manifest — the dialog then offers the explicit opt-in.
  const [needsUnverifiedConsent, setNeedsUnverifiedConsent] = useState(false)
  const [needsUrl, setNeedsUrl] = useState(false)

  // Reset local state each time the dialog opens.
  useEffect(() => {
    if (open) {
      setUrl('')
      setInstalling(false)
      setNeedsUnverifiedConsent(false)
      setNeedsUrl(false)
    }
  }, [open])

  const handleInstall = async (allowUnverified: boolean) => {
    const trimmed = url.trim()
    if (!trimmed) {
      setNeedsUrl(true)
      return
    }
    setInstalling(true)
    try {
      const result = await api.installPluginFromGit(trimmed, allowUnverified)
      toast.success(
        intl.formatMessage(
          { id: 'extensions.plugins.installSuccess' },
          { name: result.name },
        ),
      )
      if (result.warnings.length > 0) {
        toast.warning(result.warnings.join('\n'))
      }
      onInstalled(result)
      onClose()
    } catch (e) {
      console.error('Plugin install from git error:', e)
      const message = safeErrorMessage(e, 'install failed')
      if (!allowUnverified && /allow_unverified|unverified/i.test(message)) {
        setNeedsUnverifiedConsent(true)
        return
      }
      toast.error(
        intl.formatMessage(
          { id: 'extensions.plugins.installError' },
          { error: message },
        ),
      )
    } finally {
      setInstalling(false)
    }
  }

  return (
    <Modal
      open={open}
      onClose={onClose}
      size="md"
      title={t('extensions.plugins.addGit.title')}
      busy={installing}
      closeLabel={t('extensions.installDialog.closeAria')}
    >
      <ModalBody className="flex flex-col gap-md">
        <label className="flex flex-col gap-xs">
          <span className="text-label-md font-bold text-on-surface">
            {t('extensions.plugins.addGit.urlLabel')}
          </span>
          <Input
            value={url}
            onChange={(e) => {
              setUrl(e.target.value)
              setNeedsUrl(false)
              // SEC-1 (review fix): the refusal armed the opt-in for one
              // specific remote. Editing the URL aims the install at a
              // different repo, so the armed consent is withdrawn — a new
              // refusal from the new remote must re-arm it.
              setNeedsUnverifiedConsent(false)
            }}
            placeholder={t('extensions.plugins.addGit.urlPlaceholder')}
            aria-label={t('extensions.plugins.addGit.urlLabel')}
            data-testid="add-git-url-input"
            autoFocus
            disabled={installing}
            className="font-mono"
          />
        </label>
        <p className="text-label-sm text-on-surface-variant">
          {t('extensions.plugins.addGit.hint')}
        </p>
        {needsUrl && (
          <p
            data-testid="add-git-needs-url"
            className="text-label-sm text-error"
          >
            {t('extensions.plugins.addGit.needsUrl')}
          </p>
        )}
        {needsUnverifiedConsent && (
          <p
            data-testid="add-git-unverified-warning"
            className="text-label-sm text-on-warning-container bg-warning-container/40 rounded-md px-sm py-xs"
          >
            <FormattedMessage id="extensions.installDialog.bundle.unverifiedBlocked" />
          </p>
        )}
      </ModalBody>
      <ModalFooter className="pt-0">
        <Button
          variant="ghost"
          disabled={installing}
          onClick={onClose}
          className="px-md py-sm rounded-xl text-on-surface-variant hover:bg-surface-container cursor-pointer"
        >
          {t('extensions.plugins.addGit.cancel')}
        </Button>
        <Button
          type="button"
          data-testid="add-git-install"
          disabled={installing || needsUnverifiedConsent}
          onClick={() => handleInstall(false)}
          className="px-md py-sm rounded-xl bg-primary hover:bg-primary/90 text-on-primary cursor-pointer disabled:opacity-50"
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
            data-testid="add-git-install-unverified"
            disabled={installing}
            onClick={() => handleInstall(true)}
            className="px-md py-sm rounded-xl cursor-pointer disabled:opacity-60"
          >
            <span className="material-symbols-outlined icon-sm">gpp_maybe</span>
            <FormattedMessage id="extensions.installDialog.bundle.installAnyway" />
          </Button>
        )}
      </ModalFooter>
    </Modal>
  )
}
