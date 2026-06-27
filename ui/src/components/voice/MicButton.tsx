import { useIntl } from 'react-intl'
import type { VoiceState } from '@/hooks/useVoice'

interface MicButtonProps {
  state: VoiceState
  disabled?: boolean
  onStart: () => void
  onStop: () => void
}

export function MicButton({ state, disabled, onStart, onStop }: MicButtonProps) {
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })

  const isActive = state === 'recording' || state === 'transcribing' || state === 'speaking'
  const labelKey = state === 'recording'
    ? 'voice.mic.stop.aria'
    : state === 'transcribing'
    ? 'voice.mic.transcribing.aria'
    : state === 'speaking'
    ? 'voice.mic.speaking.aria'
    : 'voice.mic.start.aria'

  return (
    <button
      type="button"
      onClick={isActive ? onStop : onStart}
      disabled={disabled || state === 'transcribing'}
      aria-pressed={isActive}
      aria-label={t(labelKey)}
      title={t(labelKey)}
      className={`relative flex items-center justify-center w-10 h-10 rounded-xl transition-all focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary/30 ${
        isActive
          ? 'bg-error text-on-error shadow-md shadow-error/30'
          : 'text-on-surface-variant hover:text-primary hover:bg-surface-container'
      } disabled:opacity-40 disabled:cursor-not-allowed`}
    >
      <span
        aria-hidden="true"
        className={`material-symbols-outlined icon-md ${state === 'recording' ? 'animate-pulse' : ''}`}
      >
        {state === 'recording' ? 'stop_circle' : state === 'transcribing' ? 'hourglass_empty' : state === 'speaking' ? 'graphic_eq' : 'mic'}
      </span>
      {state === 'recording' && (
        <span
          aria-hidden="true"
          className="absolute inset-0 rounded-xl ring-2 ring-error/40 animate-ping"
        />
      )}
    </button>
  )
}
