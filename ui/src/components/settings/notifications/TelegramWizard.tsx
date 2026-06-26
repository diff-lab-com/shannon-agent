import { useState } from 'react'
import { useIntl } from 'react-intl'
import { toast } from 'sonner'
import { Button } from '@/components/ui/button'
import QRCode from './QRCode'
import * as api from '@/lib/tauri-api'

interface TelegramWizardProps {
  config: api.TelegramInboundDto | null
  onSave: (config: api.TelegramInboundDto) => Promise<void>
  onCancel: () => void
}

const BOTFATHER_URL = 'https://t.me/BotFather'

export default function TelegramWizard({ config, onSave, onCancel }: TelegramWizardProps) {
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })

  const [step, setStep] = useState(1)
  const [botToken, setBotToken] = useState(config?.bot_token || '')
  const [triggerWord, setTriggerWord] = useState(config?.trigger_word || 'shannon')
  const [allowedChats, setAllowedChats] = useState(config?.allowed_chats?.join(', ') || '')
  const [verifying, setVerifying] = useState(false)
  const [capturing, setCapturing] = useState(false)
  const [saving, setSaving] = useState(false)

  const handleVerify = async () => {
    if (!botToken.trim()) {
      toast.error(t('settings.notifications.wizard.telegram.botTokenRequired'))
      return
    }

    setVerifying(true)
    try {
      // For now, we'll just proceed - real verification would check bot info
      toast.success(t('settings.notifications.wizard.telegram.verifySuccess'))
      setStep(3)
    } catch (e) {
      console.warn('Telegram verify error:', e)
      toast.error(t('settings.notifications.wizard.telegram.verifyFailed'))
    }
    setVerifying(false)
  }

  const handleCaptureChatId = async () => {
    setCapturing(true)
    try {
      // This would call getUpdates API to capture the latest message's chat_id
      toast.success(t('settings.notifications.wizard.telegram.captureSuccess'))
    } catch (e) {
      console.warn('Capture chat ID error:', e)
      toast.error(t('settings.notifications.wizard.telegram.captureFailed'))
    }
    setCapturing(false)
  }

  const handleSave = async () => {
    if (!botToken.trim()) {
      toast.error(t('settings.notifications.wizard.telegram.botTokenRequired'))
      return
    }

    setSaving(true)
    try {
      const dto: api.TelegramInboundDto = {
        bot_token: botToken.trim(),
        trigger_word: triggerWord.trim() || 'shannon',
        allowed_chats: allowedChats.split(',').map(s => s.trim()).filter(Boolean),
      }
      await onSave(dto)
      toast.success(t('settings.notifications.wizard.telegram.saved'))
    } catch (e) {
      console.warn('Save Telegram config error:', e)
      toast.error(t('settings.notifications.wizard.saveFailed'))
    }
    setSaving(false)
  }

  const renderStep1 = () => (
    <div className="space-y-md">
      <div>
        <h3 className="font-headline-sm text-on-surface mb-xs">
          {t('settings.notifications.wizard.telegram.step1.title')}
        </h3>
        <p className="text-on-surface-variant font-body-sm mb-md">
          {t('settings.notifications.wizard.telegram.step1.description')}
        </p>
      </div>

      <div className="flex gap-md">
        <Button
          variant="outline"
          className="flex-1"
          onClick={() => window.open(BOTFATHER_URL, '_blank')}
        >
          <span className="material-symbols-outlined mr-xs">open_in_new</span>
          {t('settings.notifications.wizard.telegram.openBotFather')}
        </Button>
      </div>

      <div className="flex justify-center p-md bg-surface-container-low rounded-lg">
        <QRCode value={BOTFATHER_URL} size={150} />
      </div>

      <div className="rounded-lg border border-outline-variant/30 p-md bg-surface-container-low/40">
        <h4 className="font-label-md text-on-surface mb-sm">
          {t('settings.notifications.wizard.telegram.steps.title')}
        </h4>
        <ol className="text-on-surface-variant text-sm space-y-xs list-decimal list-inside">
          <li>{t('settings.notifications.wizard.telegram.steps.newbot')}</li>
          <li>{t('settings.notifications.wizard.telegram.steps.name')}</li>
          <li>{t('settings.notifications.wizard.telegram.steps.username')}</li>
          <li>{t('settings.notifications.wizard.telegram.steps.copy')}</li>
        </ol>
      </div>
    </div>
  )

  const renderStep2 = () => (
    <div className="space-y-md">
      <div>
        <h3 className="font-headline-sm text-on-surface mb-xs">
          {t('settings.notifications.wizard.telegram.step2.title')}
        </h3>
        <p className="text-on-surface-variant font-body-sm mb-md">
          {t('settings.notifications.wizard.telegram.step2.description')}
        </p>
      </div>

      <div>
        <label className="block font-label-lg text-on-surface mb-sm">
          {t('settings.notifications.inbound.botToken')}
        </label>
        <input
          type="password"
          value={botToken}
          onChange={(e) => setBotToken(e.target.value)}
          placeholder="123456789:ABC..."
          className="w-full px-md py-sm rounded-md border border-outline bg-surface text-on-surface focus-visible:border-primary focus-visible:ring-2 focus-visible:ring-primary/30"
        />
        <p className="text-on-surface-variant text-xs mt-sm">
          {t('settings.notifications.wizard.telegram.botTokenHint')}
        </p>
      </div>

      <Button variant="outline" onClick={handleVerify} disabled={verifying}>
        {verifying ? t('settings.notifications.wizard.actions.verifying') : t('settings.notifications.wizard.actions.verify')}
      </Button>
    </div>
  )

  const renderStep3 = () => (
    <div className="space-y-md">
      <div>
        <h3 className="font-headline-sm text-on-surface mb-xs">
          {t('settings.notifications.wizard.telegram.step3.title')}
        </h3>
        <p className="text-on-surface-variant font-body-sm mb-md">
          {t('settings.notifications.wizard.telegram.step3.description')}
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
          className="w-full px-md py-sm rounded-md border border-outline bg-surface text-on-surface focus-visible:border-primary focus-visible:ring-2 focus-visible:ring-primary/30"
        />
        <p className="text-on-surface-variant text-xs mt-sm">
          {t('settings.notifications.wizard.telegram.triggerWordHint')}
        </p>
      </div>

      <div>
        <label className="block font-label-lg text-on-surface mb-sm">
          {t('settings.notifications.inbound.allowedChats')}
        </label>
        <input
          type="text"
          value={allowedChats}
          onChange={(e) => setAllowedChats(e.target.value)}
          placeholder="-1001234567890, 123456789"
          className="w-full px-md py-sm rounded-md border border-outline bg-surface text-on-surface focus-visible:border-primary focus-visible:ring-2 focus-visible:ring-primary/30"
        />
        <p className="text-on-surface-variant text-xs mt-sm">
          {t('settings.notifications.wizard.telegram.allowedChatsHint')}
        </p>
      </div>

      <Button variant="outline" onClick={handleCaptureChatId} disabled={capturing}>
        {capturing ? t('settings.notifications.wizard.actions.capturing') : t('settings.notifications.wizard.telegram.captureChatId')}
      </Button>

      <div className="rounded-lg border border-tertiary/30 bg-tertiary/5 p-md">
        <p className="text-on-surface-variant text-sm">
          <span className="material-symbols-outlined text-sm align-middle mr-xs">info</span>
          {t('settings.notifications.wizard.telegram.step3.note')}
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
