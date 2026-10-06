import { useIntl } from 'react-intl'
import { Button } from '@/components/ui/button'
import { cn } from '@/lib/utils'
import type { VoiceState } from '@/hooks/useVoice'

interface MicButtonProps {
  state: VoiceState
  disabled?: boolean
  onStart: () => void
  onStop: () => void
}

// B4 P2-5: rendered only while the STT provider is supported (ChatInput
// gates the mount) — the TTS `speaking` state was removed with tts.ts.
export function MicButton({ state, disabled, onStart, onStop }: MicButtonProps) {
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })

  const isActive = state === 'recording' || state === 'transcribing'
  const labelKey = state === 'recording'
    ? 'voice.mic.stop.aria'
    : state === 'transcribing'
    ? 'voice.mic.transcribing.aria'
    : 'voice.mic.start.aria'

  return (
    <Button
      variant="ghost"
      onClick={isActive ? onStop : onStart}
      disabled={disabled || state === 'transcribing'}
      aria-pressed={isActive}
      aria-label={t(labelKey)}
      title={t(labelKey)}
      className={cn(
        'relative w-10 h-10 rounded-xl p-0',
        isActive
          ? 'bg-error text-on-error shadow-md shadow-error/30 hover:bg-error/90'
          : 'text-on-surface-variant hover:text-primary hover:bg-surface-container'
      )}
    >
      <span
        aria-hidden="true"
        className={cn("material-symbols-outlined icon-md", state === 'recording' && 'animate-pulse')}
      >
        {state === 'recording' ? 'stop_circle' : state === 'transcribing' ? 'hourglass_empty' : 'mic'}
      </span>
      {state === 'recording' && (
        <span
          aria-hidden="true"
          className="absolute inset-0 rounded-xl ring-2 ring-error/40 animate-ping"
        />
      )}
    </Button>
  )
}
