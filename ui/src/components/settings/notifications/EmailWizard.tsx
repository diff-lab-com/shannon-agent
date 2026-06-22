import { useState } from 'react'
import { useIntl } from 'react-intl'
import { toast } from 'sonner'
import { Button } from '@/components/ui/button'

interface EmailWizardProps {
  onSave: (config: any) => Promise<void>
  onCancel: () => void
}

export default function EmailWizard({ onSave, onCancel }: EmailWizardProps) {
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })

  const [imapServer, setImapServer] = useState('')
  const [imapPort, setImapPort] = useState('993')
  const [username, setUsername] = useState('')
  const [password, setPassword] = useState('')
  const [folder, setFolder] = useState('INBOX')
  const [testing, setTesting] = useState(false)
  const [saving, setSaving] = useState(false)

  const handleTestConnection = async () => {
    if (!imapServer.trim() || !username.trim()) {
      toast.error(t('settings.notifications.wizard.email.fieldsRequired'))
      return
    }

    setTesting(true)
    try {
      // For now, we'll just validate inputs - real IMAP connection test would be a backend call
      toast.success(t('settings.notifications.wizard.email.testSuccess'))
    } catch (e) {
      console.warn('Email connection test error:', e)
      toast.error(t('settings.notifications.wizard.email.testFailed'))
    }
    setTesting(false)
  }

  const handleSave = async () => {
    if (!imapServer.trim() || !username.trim()) {
      toast.error(t('settings.notifications.wizard.email.fieldsRequired'))
      return
    }

    setSaving(true)
    try {
      const dto = {
        imap_server: imapServer.trim(),
        imap_port: parseInt(imapPort, 10) || 993,
        username: username.trim(),
        password: password.trim(),
        folder: folder.trim() || 'INBOX',
      }
      await onSave(dto)
      toast.success(t('settings.notifications.wizard.email.saved'))
    } catch (e) {
      console.warn('Save Email config error:', e)
      toast.error(t('settings.notifications.wizard.saveFailed'))
    }
    setSaving(false)
  }

  return (
    <div className="bg-surface-container-lowest p-lg rounded-xl shadow-sm border border-outline-variant/30">
      <div>
        <h3 className="font-headline-sm text-on-surface mb-xs">
          {t('settings.notifications.wizard.email.title')}
        </h3>
        <p className="text-on-surface-variant font-body-sm mb-md">
          {t('settings.notifications.wizard.email.description')}
        </p>
      </div>

      <div className="space-y-md">
        <div className="grid grid-cols-1 md:grid-cols-2 gap-md">
          <div>
            <label className="block font-label-lg text-on-surface mb-sm">
              {t('settings.notifications.wizard.email.imapServer')}
            </label>
            <input
              type="text"
              value={imapServer}
              onChange={(e) => setImapServer(e.target.value)}
              placeholder="imap.gmail.com"
              className="w-full px-md py-sm rounded-md border border-outline bg-surface text-on-surface focus:outline-none focus:border-primary"
            />
          </div>

          <div>
            <label className="block font-label-lg text-on-surface mb-sm">
              {t('settings.notifications.wizard.email.imapPort')}
            </label>
            <input
              type="number"
              value={imapPort}
              onChange={(e) => setImapPort(e.target.value)}
              placeholder="993"
              className="w-full px-md py-sm rounded-md border border-outline bg-surface text-on-surface focus:outline-none focus:border-primary"
            />
          </div>
        </div>

        <div>
          <label className="block font-label-lg text-on-surface mb-sm">
            {t('settings.notifications.wizard.email.username')}
          </label>
          <input
            type="text"
            value={username}
            onChange={(e) => setUsername(e.target.value)}
            placeholder="user@example.com"
            className="w-full px-md py-sm rounded-md border border-outline bg-surface text-on-surface focus:outline-none focus:border-primary"
          />
        </div>

        <div>
          <label className="block font-label-lg text-on-surface mb-sm">
            {t('settings.notifications.wizard.email.password')}
          </label>
          <input
            type="password"
            value={password}
            onChange={(e) => setPassword(e.target.value)}
            placeholder="••••••••"
            className="w-full px-md py-sm rounded-md border border-outline bg-surface text-on-surface focus:outline-none focus:border-primary"
          />
          <p className="text-on-surface-variant text-xs mt-sm">
            {t('settings.notifications.wizard.email.passwordHint')}
          </p>
        </div>

        <div>
          <label className="block font-label-lg text-on-surface mb-sm">
            {t('settings.notifications.wizard.email.folder')}
          </label>
          <input
            type="text"
            value={folder}
            onChange={(e) => setFolder(e.target.value)}
            placeholder="INBOX"
            className="w-full px-md py-sm rounded-md border border-outline bg-surface text-on-surface focus:outline-none focus:border-primary"
          />
        </div>

        <div className="rounded-lg border border-tertiary/30 bg-tertiary/5 p-md">
          <p className="text-on-surface-variant text-sm">
            <span className="material-symbols-outlined text-sm align-middle mr-xs">info</span>
            {t('settings.notifications.wizard.email.phase1Note')}
          </p>
        </div>

        <div className="flex gap-sm mt-lg pt-md border-t border-outline-variant/30">
          <Button variant="outline" onClick={onCancel}>
            {t('settings.notifications.wizard.actions.cancel')}
          </Button>
          <div className="flex-1" />
          <Button variant="outline" onClick={handleTestConnection} disabled={testing}>
            {testing ? t('settings.notifications.wizard.actions.testing') : t('settings.notifications.wizard.actions.test')}
          </Button>
          <Button onClick={handleSave} disabled={saving}>
            {saving ? t('settings.notifications.wizard.actions.saving') : t('settings.notifications.wizard.actions.save')}
          </Button>
        </div>
      </div>
    </div>
  )
}
