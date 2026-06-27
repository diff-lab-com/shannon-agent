import type { VoiceState } from '@/hooks/useVoice'

interface VoiceOrbProps {
  state: VoiceState
  size?: number
}

export function VoiceOrb({ state, size = 64 }: VoiceOrbProps) {
  const baseColor = state === 'recording'
    ? 'bg-error/80'
    : state === 'speaking'
    ? 'bg-primary'
    : 'bg-primary/40'
  const ringClass = state === 'recording'
    ? 'before:bg-error/30 animate-pulse'
    : state === 'speaking'
    ? 'before:bg-primary/40 before:animate-ping'
    : 'before:bg-primary/20'

  return (
    <div
      role="presentation"
      aria-hidden="true"
      className={`relative rounded-full ${baseColor} ${ringClass} before:absolute before:inset-0 before:rounded-full before:-z-10 transition-colors`}
      style={{ width: size, height: size }}
    >
      <div className="absolute inset-2 rounded-full bg-surface-container-lowest/40 backdrop-blur-sm flex items-center justify-center">
        <span className="material-symbols-outlined text-on-surface">
          {state === 'recording' ? 'mic' : state === 'speaking' ? 'graphic_eq' : 'auto_awesome'}
        </span>
      </div>
    </div>
  )
}
