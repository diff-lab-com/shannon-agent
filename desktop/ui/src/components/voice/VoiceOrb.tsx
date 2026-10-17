import type { VoiceState } from '@/hooks/useVoice'
import { cn } from '@/lib/utils'

interface VoiceOrbProps {
  state: VoiceState
  size?: number
  /**
   * AI-chat-ux V5 — live capture loudness (0..1, ~10 Hz from useVoice).
   * Drives the halo's opacity while recording so the strip visibly reacts
   * to the voice; absent/0 rests at the baseline pulse.
   */
  level?: number
}

// B4 P2-5: STT-only states — the TTS `speaking` branch was removed with
// lib/voice/tts.ts (no reachable caller).
export function VoiceOrb({ state, size = 64, level = 0 }: VoiceOrbProps) {
  const baseColor = state === 'recording' ? 'bg-error/80' : 'bg-primary/40'
  const ringClass = state === 'recording'
    ? 'before:bg-error/30 animate-pulse'
    : 'before:bg-primary/20'
  // V5: the halo tracks the mic input — a muted mic sits near the baseline
  // pulse, a loud one visibly breathes. Purely visual (orb is aria-hidden).
  const halo = state === 'recording' ? Math.min(1, Math.max(0, level)) : 0

  return (
    <div
      role="presentation"
      aria-hidden="true"
      className={cn("relative rounded-full", baseColor, ringClass, "before:absolute before:inset-0 before:rounded-full before:-z-raised transition-colors")}
      style={{ width: size, height: size }}
    >
      {state === 'recording' && halo > 0 && (
        <div
          className="absolute inset-0 rounded-full bg-error/40"
          style={{ opacity: 0.2 + halo * 0.6, transform: `scale(${1 + halo * 0.25})` }}
        />
      )}
      {/* G1: solid disc — a 40px blur behind an icon costs a backdrop layer
          for an effect invisible at this size. */}
      <div className="absolute inset-2 rounded-full bg-surface-container-lowest/90 flex items-center justify-center">
        <span className="material-symbols-outlined text-on-surface">
          {state === 'recording' ? 'mic' : 'auto_awesome'}
        </span>
      </div>
    </div>
  )
}
