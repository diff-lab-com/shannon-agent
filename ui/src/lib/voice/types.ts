/**
 * Voice provider abstraction (Phase 3 scaffold).
 *
 * The goal is to let useVoice stay provider-agnostic so we can swap
 * between local Web Speech API, a remote Whisper endpoint, or a
 * cloud STT (Deepgram / AssemblyAI) without touching call sites.
 *
 * Phase 2 (current): useVoice uses Web Speech directly.
 * Phase 3 (this file): introduce the interface and concrete providers
 *                     behind a factory; refactor useVoice to consume it.
 */

export type VoiceProviderKind = 'stub' | 'webspeech' | 'remote'

export interface VoiceProviderConfig {
  /** Which provider implementation to use. */
  kind: VoiceProviderKind
  /** BCP-47 language code passed to the underlying engine. */
  lang?: string
  /**
   * Endpoint URL for the `remote` provider. Audio chunks are POSTed as
   * binary blobs with `Content-Type: application/octet-stream`.
   * Must use the `https:` scheme — sending bearer tokens or audio
   * over plain HTTP leaks them on the network. Providers reject
   * insecure URLs with error code `insecure-protocol`.
   */
  remoteEndpoint?: string
  /** Optional bearer token sent as `Authorization: Bearer …` over HTTPS. */
  remoteAuthToken?: string
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
  /** True when the runtime supports this provider (e.g. browser has Web Speech). */
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
