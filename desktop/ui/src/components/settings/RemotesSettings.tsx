import { useEffect, useState } from 'react'
import { useIntl } from 'react-intl'
import { toast } from 'sonner'

import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from '@/components/ui/card'
import { ConfirmDialog } from '@/components/ui/confirm-dialog'
import { Icon } from '@/components/ui/icon'
import { Input } from '@/components/ui/input'
import { Modal, ModalBody, ModalFooter } from '@/components/ui/modal'
import { toastError } from '@/lib/errorToast'
import * as api from '@/lib/tauri-api'
import type { RemoteHealth, RemoteTarget as RemoteTargetItem } from './remotes-settings/types'

/**
 * Remotes settings — manage SSH hosts and Docker containers that Shannon
 * tools can execute on. Mirrors the ConnectionsSettings card pattern.
 *
 * Security: the forms carry no credential fields; authentication rides on
 * the system ssh (`~/.ssh/config` + agent).
 */

type HealthByTarget = Record<string, RemoteHealth>

function RemotesSettings(): React.JSX.Element {
  const intl = useIntl()
  const t = (id: string): string => intl.formatMessage({ id })
  const tVal = (id: string, values: Record<string, string | number>): string =>
    intl.formatMessage({ id }, values)

  const [targets, setTargets] = useState<RemoteTargetItem[]>([])
  const [defaultTarget, setDefaultTarget] = useState<string | null>(null)
  const [loaded, setLoaded] = useState(false)
  const [dialogOpen, setDialogOpen] = useState(false)
  const [removeTarget, setRemoveTarget] = useState<RemoteTargetItem | null>(null)
  const [health, setHealth] = useState<HealthByTarget>({})
  const [testing, setTesting] = useState<string | null>(null)

  async function reload(): Promise<void> {
    try {
      const list = await api.remoteListTargets()
      setTargets(list)
      // The default is stored in remotes.toml; surface it if present.
      setDefaultTarget((prev) => prev ?? null)
      setLoaded(true)
    } catch (e) {
      toastError('remotes: load failed', e)
      setLoaded(true)
    }
  }

  useEffect(() => {
    void reload()
  }, [])

  async function runTest(target: RemoteTargetItem): Promise<void> {
    setTesting(target.name)
    try {
      const result = await api.remoteTestTarget(target.name)
      setHealth((prev) => ({ ...prev, [target.name]: result }))
      if (!result.ok && result.error) {
        toast.error(tVal('settings.remotes.testFailedToast', { name: target.name }))
      }
    } catch (e) {
      toastError('remotes: test failed', e)
    } finally {
      setTesting(null)
    }
  }

  async function makeDefault(target: RemoteTargetItem): Promise<void> {
    try {
      await api.remoteSetDefaultTarget(target.name)
      setDefaultTarget(target.name)
      toast.success(tVal('settings.remotes.defaultSet', { name: target.name }))
    } catch (e) {
      toastError('remotes: set default failed', e)
    }
  }

  async function clearDefault(): Promise<void> {
    try {
      await api.remoteSetDefaultTarget(null)
      setDefaultTarget(null)
      toast.success(t('settings.remotes.defaultCleared'))
    } catch (e) {
      toastError('remotes: clear default failed', e)
    }
  }

  async function confirmRemove(): Promise<void> {
    if (!removeTarget) return
    try {
      await api.remoteRemoveTarget(removeTarget.name)
      toast.success(tVal('settings.remotes.removeDone', { name: removeTarget.name }))
      setRemoveTarget(null)
      await reload()
    } catch (e) {
      toastError('remotes: remove failed', e)
    }
  }

  return (
    <div className="space-y-md" data-testid="remotes-settings">
      <div>
        <h2 className="text-headline-sm font-medium">{t('settings.remotes.title')}</h2>
        <p className="text-body-sm text-on-surface-variant">{t('settings.remotes.description')}</p>
      </div>

      {loaded && targets.length === 0 && (
        <Card>
          <CardContent>
            <div
              className="flex flex-col items-center gap-sm py-lg text-center"
              data-testid="remotes-empty"
            >
              <Icon name="dns" className="text-on-surface-variant" />
              <p className="text-body-md">{t('settings.remotes.emptyTitle')}</p>
              <p className="text-body-sm text-on-surface-variant">
                {t('settings.remotes.emptyDescription')}
              </p>
              <Button onClick={() => setDialogOpen(true)} data-testid="remotes-empty-add">
                {t('settings.remotes.add')}
              </Button>
            </div>
          </CardContent>
        </Card>
      )}

      {targets.length > 0 && (
        <Card data-testid="remotes-targets-card">
          <CardHeader>
            <CardTitle>{t('settings.remotes.targetsTitle')}</CardTitle>
            <CardDescription>{t('settings.remotes.targetsDescription')}</CardDescription>
          </CardHeader>
          <CardContent className="space-y-sm">
            {targets.map((target) => {
              const h = health[target.name]
              return (
                <div
                  key={target.name}
                  className="flex items-center gap-sm rounded border p-sm"
                  data-testid={`remotes-target-${target.name}`}
                >
                  <Icon name={target.kind === 'ssh' ? 'terminal' : 'deployed_code'} />
                  <div className="min-w-0 flex-1">
                    <div className="flex items-center gap-sm">
                      <span className="font-medium">{target.name}</span>
                      <Badge variant="secondary">{target.kind}</Badge>
                      {defaultTarget === target.name && (
                        <Badge data-testid={`remotes-default-${target.name}`}>
                          {t('settings.remotes.defaultBadge')}
                        </Badge>
                      )}
                    </div>
                    <p className="truncate text-body-sm text-on-surface-variant">
                      {target.kind === 'ssh'
                        ? `${target.host ?? ''} · ${target.workspaceDir}`
                        : `${target.container ?? ''} · ${target.workspaceDir}`}
                    </p>
                    {h && (
                      <p className="text-body-sm" data-testid={`remotes-health-${target.name}`}>
                        {h.ok
                          ? tVal('settings.remotes.healthOk', {
                              platform: h.platform,
                              latency: h.latencyMs,
                            })
                          : t('settings.remotes.healthFail')}
                      </p>
                    )}
                  </div>
                  <div className="flex shrink-0 items-center gap-xs">
                    <Button
                      variant="outline"
                      size="sm"
                      disabled={testing === target.name}
                      onClick={() => void runTest(target)}
                      data-testid={`remotes-test-${target.name}`}
                    >
                      {testing === target.name
                        ? t('settings.remotes.testing')
                        : t('settings.remotes.test')}
                    </Button>
                    {defaultTarget !== target.name && (
                      <Button
                        variant="ghost"
                        size="sm"
                        onClick={() => void makeDefault(target)}
                        data-testid={`remotes-set-default-${target.name}`}
                      >
                        {t('settings.remotes.setDefault')}
                      </Button>
                    )}
                    <Button
                      variant="ghost"
                      size="sm"
                      onClick={() => void clearDefault()}
                      disabled={defaultTarget !== target.name}
                      data-testid={`remotes-clear-default-${target.name}`}
                    >
                      {t('settings.remotes.clearDefault')}
                    </Button>
                    <Button
                      variant="ghost"
                      size="sm"
                      onClick={() => setRemoveTarget(target)}
                      data-testid={`remotes-remove-${target.name}`}
                      aria-label={t('settings.remotes.remove')}
                    >
                      <Icon name="delete" />
                    </Button>
                  </div>
                </div>
              )
            })}
          </CardContent>
        </Card>
      )}

      {targets.length > 0 && (
        <Button onClick={() => setDialogOpen(true)} data-testid="remotes-add">
          {t('settings.remotes.add')}
        </Button>
      )}

      <AddRemoteDialog
        open={dialogOpen}
        onClose={() => setDialogOpen(false)}
        onAdded={() => {
          setDialogOpen(false)
          void reload()
        }}
      />

      <ConfirmDialog
        open={removeTarget !== null}
        onCancel={() => setRemoveTarget(null)}
        onConfirm={() => void confirmRemove()}
        title={tVal('settings.remotes.removeTitle', {
          name: removeTarget?.name ?? '',
        })}
        message={t('settings.remotes.removeDescription')}
        confirmLabel={t('settings.remotes.remove')}
        cancelLabel={t('settings.remotes.cancel')}
        destructive
      />
    </div>
  )
}

interface AddRemoteDialogProps {
  open: boolean
  onClose: () => void
  onAdded: () => void
}

/** Modal form for registering a target. No credential fields by design. */
function AddRemoteDialog({ open, onClose, onAdded }: AddRemoteDialogProps): React.JSX.Element {
  const intl = useIntl()
  const t = (id: string): string => intl.formatMessage({ id })
  const tVal = (id: string, values: Record<string, string | number>): string =>
    intl.formatMessage({ id }, values)

  const [kind, setKind] = useState<'ssh' | 'docker'>('ssh')
  const [name, setName] = useState('')
  const [detail, setDetail] = useState('')
  const [workspaceDir, setWorkspaceDir] = useState('')
  const [busy, setBusy] = useState(false)

  function reset(): void {
    setName('')
    setDetail('')
    setWorkspaceDir('')
  }

  async function submit(): Promise<void> {
    setBusy(true)
    try {
      await api.remoteAddTarget({
        name: name.trim(),
        kind,
        host: kind === 'ssh' ? detail.trim() : null,
        port: null,
        user: null,
        container: kind === 'docker' ? detail.trim() : null,
        shell: null,
        sshTarget: null,
        workspaceDir: workspaceDir.trim(),
      })
      toast.success(tVal('settings.remotes.added', { name: name.trim() }))
      reset()
      onAdded()
    } catch (e) {
      toastError('remotes: add failed', e)
    } finally {
      setBusy(false)
    }
  }

  const valid =
    name.trim().length > 0 && detail.trim().length > 0 && workspaceDir.trim().startsWith('/')

  return (
    <Modal
      open={open}
      onClose={onClose}
      title={t('settings.remotes.dialogTitle')}
      description={t('settings.remotes.dialogDescription')}
    >
      <ModalBody className="space-y-sm">
        <label className="block space-y-xs">
          <span className="text-label-md">{t('settings.remotes.fieldKind')}</span>
          <select
            className="h-8 w-full min-w-0 rounded-lg border border-input bg-transparent px-2.5 py-1 text-base transition-colors outline-none focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50 md:text-sm dark:bg-input/30"
            value={kind}
            onChange={(e) => setKind(e.target.value as 'ssh' | 'docker')}
            data-testid="remotes-dialog-kind"
          >
            <option value="ssh">SSH</option>
            <option value="docker">Docker</option>
          </select>
        </label>
        <label className="block space-y-xs">
          <span className="text-label-md">{t('settings.remotes.fieldName')}</span>
          <Input
            value={name}
            onChange={(e) => setName(e.target.value)}
            data-testid="remotes-dialog-name"
          />
        </label>
        <label className="block space-y-xs">
          <span className="text-label-md">
            {kind === 'ssh'
              ? t('settings.remotes.fieldHost')
              : t('settings.remotes.fieldContainer')}
          </span>
          <Input
            value={detail}
            onChange={(e) => setDetail(e.target.value)}
            data-testid="remotes-dialog-detail"
          />
        </label>
        <label className="block space-y-xs">
          <span className="text-label-md">{t('settings.remotes.fieldWorkspace')}</span>
          <Input
            value={workspaceDir}
            onChange={(e) => setWorkspaceDir(e.target.value)}
            placeholder="/home/user/project"
            data-testid="remotes-dialog-workspace"
          />
        </label>
      </ModalBody>
      <ModalFooter>
        <Button variant="ghost" onClick={onClose}>
          {t('settings.remotes.cancel')}
        </Button>
        <Button disabled={!valid || busy} onClick={() => void submit()} data-testid="remotes-dialog-submit">
          {busy ? t('settings.remotes.saving') : t('settings.remotes.save')}
        </Button>
      </ModalFooter>
    </Modal>
  )
}

export default RemotesSettings
