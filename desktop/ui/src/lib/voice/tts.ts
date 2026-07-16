/**
 * Text-to-speech wrapper around `window.speechSynthesis`.
 *
 * Design goals:
 *  - Single speak() entrypoint that cancels any in-flight utterance
 *    so rapid retries don't queue up.
 *  - Voice matching by URI (stable across reloads) with lang fallback.
 *  - Lifecycle observable via onStateChange callback so React can
 *    drive orb animation / button state without polling.
 *  - Safe no-op when the API is missing (jsdom, older WebKit).
 */
export type TtsState = 'idle' | 'speaking' | 'paused'

export interface TtsOptions {
  /** BCP-47 lang code passed to the utterance. */
  lang?: string
  /** Preferred voice URI. Falls back to first matching-lang voice. */
  voiceURI?: string
  /** 0.1 – 10; default 1. */
  rate?: number
  /** 0 – 2; default 1. */
  pitch?: number
  /** 0 – 1; default 1. */
  volume?: number
  /** Fired whenever the engine transitions between idle/speaking/paused. */
  onStateChange?: (state: TtsState) => void
  /** Fired when an utterance errors (e.g. interrupted). */
  onError?: (err: string) => void
}

export interface TtsSpeaker {
  readonly state: TtsState
  isSupported(): boolean
  speak(text: string): void
  pause(): void
  resume(): void
  cancel(): void
}

export function isTtsSupported(): boolean {
  return typeof window !== 'undefined' && typeof window.speechSynthesis !== 'undefined'
}

export function listVoices(): SpeechSynthesisVoice[] {
  if (!isTtsSupported()) return []
  return window.speechSynthesis.getVoices()
}

export function pickVoice(lang: string, voiceURI?: string): SpeechSynthesisVoice | null {
  const voices = listVoices()
  if (voices.length === 0) return null
  if (voiceURI) {
    const exact = voices.find(v => v.voiceURI === voiceURI)
    if (exact) return exact
  }
  const langMatch = voices.find(v => v.lang.toLowerCase().startsWith(lang.toLowerCase()))
  return langMatch ?? voices[0]
}

export function createTtsSpeaker(options: TtsOptions = {}): TtsSpeaker {
  const lang = options.lang ?? 'en-US'
  const rate = clamp(options.rate ?? 1, 0.1, 10)
  const pitch = clamp(options.pitch ?? 1, 0, 2)
  const volume = clamp(options.volume ?? 1, 0, 1)

  let currentState: TtsState = 'idle'

  const setState = (next: TtsState) => {
    if (currentState === next) return
    currentState = next
    options.onStateChange?.(next)
  }

  const cancel = () => {
    if (!isTtsSupported()) return
    try { window.speechSynthesis.cancel() } catch { /* no-op */ }
    setState('idle')
  }

  const speak = (text: string) => {
    if (!isTtsSupported() || !text) {
      if (!text) cancel()
      return
    }
    // Replace any in-flight utterance — speechSynthesis queues by default
    // and we never want stacked utterances for assistant replies.
    try { window.speechSynthesis.cancel() } catch { /* no-op */ }
    const utterance = new SpeechSynthesisUtterance(text)
    utterance.lang = lang
    utterance.rate = rate
    utterance.pitch = pitch
    utterance.volume = volume
    const voice = pickVoice(lang, options.voiceURI)
    if (voice) utterance.voice = voice
    utterance.onstart = () => setState('speaking')
    utterance.onend = () => setState('idle')
    utterance.onerror = (e) => {
      // 'interrupted' and 'canceled' fire during normal cancel() — don't surface.
      const err = (e as SpeechSynthesisErrorEvent).error
      if (err === 'interrupted' || err === 'canceled') {
        setState('idle')
        return
      }
      options.onError?.(err || 'unknown')
      setState('idle')
    }
    try {
      window.speechSynthesis.speak(utterance)
    } catch (err) {
      options.onError?.(String(err instanceof Error ? err.message : err))
      setState('idle')
    }
  }

  const pause = () => {
    if (!isTtsSupported()) return
    try { window.speechSynthesis.pause(); setState('paused') } catch { /* no-op */ }
  }

  const resume = () => {
    if (!isTtsSupported()) return
    try { window.speechSynthesis.resume(); setState('speaking') } catch { /* no-op */ }
  }

  return {
    get state() { return currentState },
    isSupported: () => isTtsSupported(),
    speak,
    pause,
    resume,
    cancel,
  }
}

function clamp(value: number, min: number, max: number): number {
  if (Number.isNaN(value)) return min
  return Math.min(max, Math.max(min, value))
}
