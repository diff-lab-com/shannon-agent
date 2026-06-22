import { useState } from 'react'
import { useIntl } from 'react-intl'
import { toast } from 'sonner'
import { Button } from '@/components/ui/button'
import * as api from '@/lib/tauri-api'

interface SlackWizardProps {
  config: api.SlackInboundDto | null
  onSave: (config: api.SlackInboundDto) => Promise<void>
  onCancel: () => void
}

export default function SlackWizard({ config, onSave, onCancel }: SlackWizardProps) {
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })

  const [step, setStep] = useState(1)
  const [botToken, setBotToken] = useState(config?.bot_token || '')
  const [triggerWord, setTriggerWord] = useState(config?.trigger_word || 'shannon')
  const [allowedChannels, setAllowedChannels] = useState(config?.allowed_channels?.join(', ') || '')
  const [saving, setSaving] = useState(false)

  const slackScopes = [
    'chat:write',
    'app_mentions:read',
    'channels:history',
    'groups:history',
    'im:history',
    'mpim:history',
  ]

  const handleCopyManifest = () => {
    const manifest = {
      display_information: {
        name: 'Shannon Assistant',
        description: 'AI-powered assistant for your workspace',
        background_color: '#2C2D33',
      },
      features: {
        bot_user: {
          display_name: 'Shannon',
          default_icon_url: 'https://platform.slack-edge.com/img/default_application_icon.png',
        },
        app_home: {
          home_tab_enabled: true,
          messages_tab_enabled: true,
          messages_tab_read_only_enabled: false,
        },
      },
      oauth_config: {
        scopes: {
          bot: slackScopes,
        },
        redirect_urls: [],
      },
      socket_mode: true,
    }

    navigator.clipboard.writeText(JSON.stringify(manifest, null, 2))
    toast.success(t('settings.notifications.wizard.slack.manifestCopied'))
  }

  const handleCopyScopes = () => {
    navigator.clipboard.writeText(slackScopes.join(','))
    toast.success(t('settings.notifications.wizard.slack.scopesCopied'))
  }


  const handleSave = async () => {
    if (!botToken.trim()) {
      toast.error(t('settings.notifications.wizard.slack.botTokenRequired'))
      return
    }

    setSaving(true)
    try {
      const dto: api.SlackInboundDto = {
        bot_token: botToken.trim(),
        trigger_word: triggerWord.trim() || 'shannon',
        allowed_channels: allowedChannels.split(',').map(s => s.trim()).filter(Boolean),
      }
      await onSave(dto)
      toast.success(t('settings.notifications.wizard.slack.saved'))
    } catch (e) {
      console.warn('Save Slack config error:', e)
      toast.error(t('settings.notifications.wizard.saveFailed'))
    }
    setSaving(false)
  }

  const renderStep1 = () => (
    <div className="space-y-md">
      <div>
        <h3 className="font-headline-sm text-on-surface mb-xs">
          {t('settings.notifications.wizard.slack.step1.title')}
        </h3>
        <p className="text-on-surface-variant font-body-sm mb-md">
          {t('settings.notifications.wizard.slack.step1.description')}
        </p>
        <Button
          variant="outline"
          className="w-full"
          onClick={() => window.open('https://api.slack.com/apps?new_app=1', '_blank')}
        >
          <span className="material-symbols-outlined mr-xs">open_in_new</span>
          {t('settings.notifications.wizard.slack.step1.createApp')}
        </Button>
      </div>

      <div className="rounded-lg border border-outline-variant/30 p-md bg-surface-container-low/40">
        <h4 className="font-label-md text-on-surface mb-sm">
          {t('settings.notifications.wizard.slack.scopes.title')}
        </h4>
        <p className="text-on-surface-variant text-sm mb-sm">
          {t('settings.notifications.wizard.slack.scopes.description')}
        </p>
        <div className="flex flex-wrap gap-xs mb-sm">
          {slackScopes.map(scope => (
            <code key={scope} className="px-xs py-xxs rounded bg-primary/10 text-primary text-xs">
              {scope}
            </code>
          ))}
        </div>
        <Button variant="outline" size="sm" onClick={handleCopyScopes}>
          <span className="material-symbols-outlined mr-xs text-sm">content_copy</span>
          {t('settings.notifications.wizard.actions.copy')}
        </Button>
      </div>

      <div className="rounded-lg border border-outline-variant/30 p-md bg-surface-container-low/40">
        <h4 className="font-label-md text-on-surface mb-sm">
          {t('settings.notifications.wizard.slack.manifest.title')}
        </h4>
        <p className="text-on-surface-variant text-sm mb-sm">
          {t('settings.notifications.wizard.slack.manifest.description')}
        </p>
        <Button variant="outline" size="sm" onClick={handleCopyManifest}>
          <span className="material-symbols-outlined mr-xs text-sm">content_copy</span>
          {t('settings.notifications.wizard.slack.manifest.copy')}
        </Button>
      </div>
    </div>
  )

  const renderStep2 = () => (
    <div className="space-y-md">
      <div>
        <h3 className="font-headline-sm text-on-surface mb-xs">
          {t('settings.notifications.wizard.slack.step2.title')}
        </h3>
        <p className="text-on-surface-variant font-body-sm mb-md">
          {t('settings.notifications.wizard.slack.step2.description')}
        </p>
      </div>

      <div>
        <label className="block font-label-lg text-on-surface mb-sm">
          {t('settings.notifications.wizard.slack.botToken')}
        </label>
        <input
          type="password"
          value={botToken}
          onChange={(e) => setBotToken(e.target.value)}
          placeholder="xoxb-..."
          className="w-full px-md py-sm rounded-md border border-outline bg-surface text-on-surface focus:outline-none focus:border-primary"
        />
        <p className="text-on-surface-variant text-xs mt-sm">
          {t('settings.notifications.wizard.slack.botTokenHint')}
        </p>
      </div>
    </div>
  )

  const renderStep3 = () => (
    <div className="space-y-md">
      <div>
        <h3 className="font-headline-sm text-on-surface mb-xs">
          {t('settings.notifications.wizard.slack.step3.title')}
        </h3>
        <p className="text-on-surface-variant font-body-sm mb-md">
          {t('settings.notifications.wizard.slack.step3.description')}
        </p>
      </div>

      <div>
        <label className="block font-label-lg text-on-surface mb-sm">
          {t('settings.notifications.inbound.triggerWord')}
        </label>
        <input
          type="text"
          value={triggerWord}
          onChange={(e) => setTriggerWord(e.target.value)}
          placeholder="shannon"
          className="w-full px-md py-sm rounded-md border border-outline bg-surface text-on-surface focus:outline-none focus:border-primary"
        />
        <p className="text-on-surface-variant text-xs mt-sm">
          {t('settings.notifications.wizard.slack.triggerWordHint')}
        </p>
      </div>

      <div>
        <label className="block font-label-lg text-on-surface mb-sm">
          {t('settings.notifications.inbound.allowedChannels')}
        </label>
        <input
          type="text"
          value={allowedChannels}
          onChange={(e) => setAllowedChannels(e.target.value)}
          placeholder="C012345, C678901"
          className="w-full px-md py-sm rounded-md border border-outline bg-surface text-on-surface focus:outline-none focus:border-primary"
        />
        <p className="text-on-surface-variant text-xs mt-sm">
          {t('settings.notifications.wizard.slack.allowedChannelsHint')}
        </p>
      </div>

      <div className="rounded-lg border border-tertiary/30 bg-tertiary/5 p-md">
        <p className="text-on-surface-variant text-sm">
          <span className="material-symbols-outlined text-sm align-middle mr-xs">info</span>
          {t('settings.notifications.wizard.slack.step3.note')}
        </p>
      </div>
    </div>
  )

  return (
    <div className="bg-surface-container-lowest p-lg rounded-xl shadow-sm border border-outline-variant/30">
      {/* Stepper */}
      <div className="flex items-center justify-between mb-lg">
        {[1, 2, 3].map((s) => (
          <div key={s} className="flex items-center">
            <div
              className={`w-8 h-8 rounded-full flex items-center justify-center font-label-md ${
                s <= step
                  ? 'bg-primary text-on-primary'
                  : 'bg-surface-container-high text-on-surface-variant'
              }`}
            >
              {s}
            </div>
            {s < 3 && (
              <div
                className={`w-16 h-0.5 mx-sm ${
                  s < step ? 'bg-primary' : 'bg-outline-variant'
                }`}
              />
            )}
          </div>
        ))}
      </div>

      {/* Step content */}
      <div className="min-h-[300px]">
        {step === 1 && renderStep1()}
        {step === 2 && renderStep2()}
        {step === 3 && renderStep3()}
      </div>

      {/* Navigation */}
      <div className="flex gap-sm mt-lg pt-md border-t border-outline-variant/30">
        <Button variant="outline" onClick={onCancel}>
          {t('settings.notifications.wizard.actions.cancel')}
        </Button>
        <div className="flex-1" />
        {step > 1 && (
          <Button variant="outline" onClick={() => setStep(step - 1)}>
            {t('settings.notifications.wizard.actions.back')}
          </Button>
        )}
        {step < 3 ? (
          <Button onClick={() => setStep(step + 1)}>
            {t('settings.notifications.wizard.actions.next')}
          </Button>
        ) : (
          <Button onClick={handleSave} disabled={saving}>
            {saving ? t('settings.notifications.wizard.actions.saving') : t('settings.notifications.wizard.actions.save')}
          </Button>
        )}
      </div>
    </div>
  )
}
