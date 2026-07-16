/**
 * Voice provider abstraction.
 *
 * useVoice stays provider-agnostic so we can swap the STT backend without
 * touching call sites. The current concrete providers are:
 *  - `remote`: cloud Whisper STT. Captures audio via MediaRecorder and sends
 *    it to the Rust `transcribe_audio` command (Groq / OpenAI / custom).
 *  - `stub`: deterministic fallback for environments without MediaRecorder
 *    (jsdom tests, headless webviews). Emits a fixed transcript on stop().
 *
 * The legacy Web Speech API provider was removed in favor of cloud STT; the
 * browser's SpeechRecognition API is unavailable in most desktop webviews
 * (Chromium on Linux/macOS), which made it an unreliable default.
 */

export type VoiceProviderKind = 'stub' | 'remote'

export interface VoiceProviderConfig {
  /** Which provider implementation to use. */
  kind: VoiceProviderKind
  /** BCP-47 language code hint (currently unused — cloud STT auto-detects). */
  lang?: string
}

export interface VoiceInterimResult {
  /** Partial transcript since the last final result. */
  partial: string
  /** True when the engine is confident in a final answer. */
  isFinal: boolean
}

export interface VoiceFinalResult {
  transcript: string
}

export type VoiceResultHandler = (result: VoiceInterimResult | VoiceFinalResult) => void
export type VoiceErrorHandler = (error: VoiceProviderError) => void

export interface VoiceProviderError {
  code: string
  message: string
  /** True when the failure should be silently ignored (e.g. user canceled). */
  silent?: boolean
}

export interface VoiceProvider {
  /** Unique identifier matching `VoiceProviderConfig.kind`. */
  readonly kind: VoiceProviderKind
  /** True when the runtime supports this provider (e.g. MediaRecorder present). */
  isSupported(): boolean
  /** Begin capturing audio and emitting results. */
  start(handlers: {
    onResult: VoiceResultHandler
    onError: VoiceErrorHandler
    onEnd?: () => void
  }): Promise<void>
  /** Stop capturing; the engine should emit any pending final result. */
  stop(): Promise<void>
  /** Abort immediately without emitting further results. */
  abort(): void
}
