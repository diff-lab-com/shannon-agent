import { useRef, useState } from 'react'
import { useIntl } from 'react-intl'
import { Button } from '@/components/ui/button'
import { cn } from '@/lib/utils'
import type { VoiceState } from '@/hooks/useVoice'

interface MicButtonProps {
  state: VoiceState
  disabled?: boolean
  onStart: () => void
  onStop: () => void
  /**
   * AI-chat-ux V3 — speech-to-text is not configured yet. The button stays
   * clickable (the competitor converged pattern: never a dead button) but
   * routes to the STT settings card instead of starting a recording that
   * can only fail with `STT_NOT_CONFIGURED`.
   */
  needsSetup?: boolean
  /** Settings deep-link for the needsSetup state. */
  onOpenSettings?: () => void
}

/**
 * Pointer press ≥ this threshold counts as push-to-talk (V6): the release
 * STOPS the recording. A shorter press is a plain tap — recording keeps
 * running and the next tap (or release-then-click) stops it. Same converged
 * dual-mode interaction as VS Code dictation / SuperWhisper: tap = toggle,
 * hold = talk.
 */
const PTT_THRESHOLD_MS = 350

// B4 P2-5: rendered only while the STT provider is supported (ChatInput
// gates the mount) — the TTS `speaking` state was removed with tts.ts.
export function MicButton({ state, disabled, onStart, onStop, needsSetup, onOpenSettings }: MicButtonProps) {
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })

  const isActive = state === 'recording' || state === 'transcribing'
  const labelKey = needsSetup
    ? 'voice.mic.needsSetup.title'
    : state === 'recording'
    ? 'voice.mic.stop.aria'
    : state === 'transcribing'
    ? 'voice.mic.transcribing.aria'
    : 'voice.mic.start.aria'

  // V6 push-to-talk press bookkeeping. `pressedAt == null` ⇔ no live press;
  // `pressed` mirrors it into render for the squeeze feedback;
  // `wasIdleAtPress` separates the tap that STARTED a recording (release
  // keeps it running — toggle ON) from a second tap on an already-running
  // recording (release stops it — toggle OFF), while any hold ≥ threshold
  // is pure push-to-talk (release always stops).
  const pressedAtRef = useRef<number | null>(null)
  const wasIdleAtPressRef = useRef(false)
  const [pressed, setPressed] = useState(false)

  const clearPress = () => {
    pressedAtRef.current = null
    wasIdleAtPressRef.current = false
    setPressed(false)
  }

  /**
   * Shared release logic. `holdOnly` (the slide-off path) only resolves
   * long holds — a short slide-off reads as an aborted tap and leaves an
   * initiated recording running; the full 'auto' release (pointerup) also
   * resolves the toggle-off second tap.
   */
  const releasePress = (holdOnly: boolean) => {
    const pressedAt = pressedAtRef.current
    const wasIdle = wasIdleAtPressRef.current
    clearPress()
    if (pressedAt == null || needsSetup) return
    if (state !== 'recording') return
    if (Date.now() - pressedAt >= PTT_THRESHOLD_MS) {
      void onStop()
      return
    }
    if (!holdOnly && !wasIdle) void onStop()
  }

  return (
    <Button
      variant="ghost"
      // V6: keyboard activation (Enter/Space → click with detail 0) keeps
      // the toggle contract; pointer-driven clicks (detail ≥ 1) are already
      // handled by the pointer handlers below and must not double-fire.
      onClick={(e) => {
        if (needsSetup) {
          onOpenSettings?.()
          return
        }
        if (e.detail > 0) return
        if (isActive) void onStop()
        else void onStart()
      }}
      onPointerDown={(e) => {
        if (needsSetup || disabled || state === 'transcribing') return
        // Only a secondary pointer (multi-touch) is ignored — `undefined`
        // (synthetic tests, exotic webviews) passes.
        if (e.isPrimary === false) return
        pressedAtRef.current = Date.now()
        wasIdleAtPressRef.current = !isActive
        setPressed(true)
        // Press = capture starts immediately (the push-to-talk semantic);
        // whether the release keeps or stops it is decided by hold length.
        if (!isActive) onStart()
      }}
      onPointerUp={(e) => {
        if (pressedAtRef.current != null && Date.now() - pressedAtRef.current >= PTT_THRESHOLD_MS) {
          e.preventDefault()
        }
        releasePress(false)
      }}
      onPointerLeave={() => releasePress(true)}
      disabled={disabled || state === 'transcribing'}
      aria-pressed={isActive}
      aria-label={t(labelKey)}
      title={t(labelKey)}
      data-testid={needsSetup ? 'mic-button-needs-setup' : 'mic-button'}
      className={cn(
        'relative w-10 h-10 rounded-xl p-0 select-none touch-none',
        isActive
          ? 'bg-error text-on-error shadow-md shadow-error/30 hover:bg-error/90'
          : needsSetup
            ? 'text-on-surface-variant/70 hover:text-warning hover:bg-surface-container'
            : 'text-on-surface-variant hover:text-primary hover:bg-surface-container',
        pressed && 'scale-90',
      )}
    >
      <span
        aria-hidden="true"
        className={cn("material-symbols-outlined icon-md", state === 'recording' && 'animate-pulse')}
      >
        {needsSetup ? 'mic_off' : state === 'recording' ? 'stop_circle' : state === 'transcribing' ? 'hourglass_empty' : 'mic'}
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
