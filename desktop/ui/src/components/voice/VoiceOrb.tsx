import type { VoiceState } from '@/hooks/useVoice'
import { cn } from '@/lib/utils'

interface VoiceOrbProps {
  state: VoiceState
  size?: number
}

// B4 P2-5: STT-only states — the TTS `speaking` branch was removed with
// lib/voice/tts.ts (no reachable caller).
export function VoiceOrb({ state, size = 64 }: VoiceOrbProps) {
  const baseColor = state === 'recording' ? 'bg-error/80' : 'bg-primary/40'
  const ringClass = state === 'recording'
    ? 'before:bg-error/30 animate-pulse'
    : 'before:bg-primary/20'

  return (
    <div
      role="presentation"
      aria-hidden="true"
      className={cn("relative rounded-full", baseColor, ringClass, "before:absolute before:inset-0 before:rounded-full before:-z-raised transition-colors")}
      style={{ width: size, height: size }}
    >
      <div className="absolute inset-2 rounded-full bg-surface-container-lowest/40 backdrop-blur-sm flex items-center justify-center">
        <span className="material-symbols-outlined text-on-surface">
          {state === 'recording' ? 'mic' : 'auto_awesome'}
        </span>
      </div>
    </div>
  )
}
