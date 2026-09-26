import { useState, useCallback, useRef, useEffect } from 'react'
import {
  createVoiceProvider,
  defaultVoiceConfig,
  type VoiceProvider,
  type VoiceProviderError,
} from '@/lib/voice'

// B4 P2-5: the former TTS half of this hook (speak/stopSpeaking, the
// cross-instance speaker cancellation and lib/voice/tts.ts) had zero
// callers — assistant-voice playback is deferred as a future feature.
// The hook is speech-to-text only now.

export type VoiceState = 'idle' | 'recording' | 'transcribing'

export interface UseVoiceOptions {
  onTranscript?: (text: string) => void
  /** Non-silent provider failures (rejected mic, bad key, network, …). */
  onError?: (message: string) => void
  /**
   * P2-5e: force a specific STT provider. When unset, the hook
   * falls back to the cloud provider (default). Local recordings
   * require the desktop to be built with the `voice-local` Cargo
   * feature; the backend returns `STT_FEATURE_DISABLED` when
   * that feature is missing.
   */
  provider?: 'cloud' | 'local'
  /**
   * Local-provider options (only used when `provider === 'local'`).
   * The factory passes these into `createLocalProvider`, which
   * forwards them to `transcribe_audio_local`. `null` model lets
   * the Rust side pick the smallest downloaded model.
   */
  local?: {
    model: string | null
    language?: string | null
  }
}

export interface UseVoiceResult {
  state: VoiceState
  partialTranscript: string
  error: string | null
  supported: boolean
  startRecording: () => Promise<void>
  stopRecording: () => Promise<void>
  reset: () => void
}

export function useVoice(options: UseVoiceOptions = {}): UseVoiceResult {
  const { onTranscript, onError, provider = 'cloud', local } = options
  const [state, setState] = useState<VoiceState>('idle')
  const [partialTranscript, setPartialTranscript] = useState('')
  const [error, setError] = useState<string | null>(null)

  const transcriptRef = useRef(onTranscript)
  transcriptRef.current = onTranscript
  const errorRef = useRef(onError)
  errorRef.current = onError

  // Build the provider once. The kind is resolved from
  // `options.provider` (P2-5e). When the local provider is
  // selected and MediaRecorder is unavailable, the factory
  // falls back to the stub — same fallback semantics as the
  // cloud provider.
  const providerRef = useRef<VoiceProvider | null>(null)
  if (!providerRef.current) {
    const config = provider === 'local'
      ? { kind: 'local' as const, local: local ?? { model: null, language: null } }
      : defaultVoiceConfig()
    providerRef.current = createVoiceProvider(config)
  }
  const supported = providerRef.current.isSupported()

  const handleError = useCallback((err: VoiceProviderError) => {
    if (!err.silent) {
      setError(err.message)
      errorRef.current?.(err.message)
    }
  }, [])

  useEffect(() => {
    return () => { providerRef.current?.abort() }
  }, [])

  const startRecording = useCallback(async () => {
    setError(null)
    setState('recording')
    setPartialTranscript('')
    const provider = providerRef.current
    if (!provider) return
    try {
      await provider.start({
        onResult: (result) => {
          if ('transcript' in result) {
            const text = result.transcript.trim()
            if (text) transcriptRef.current?.(text)
          } else {
            setPartialTranscript(result.partial)
          }
        },
        onError: (err) => {
          handleError(err)
          setState((s) => (s === 'recording' ? 'idle' : s))
        },
        onEnd: () => {
          setPartialTranscript('')
          setState((s) => (s === 'recording' || s === 'transcribing' ? 'idle' : s))
        },
      })
    } catch (err) {
      handleError({
        code: 'engine-error',
        message: String(err instanceof Error ? err.message : err),
      })
      setState('idle')
    }
  }, [handleError])

  const stopRecording = useCallback(async () => {
    const provider = providerRef.current
    if (!provider) return
    setState('transcribing')
    setPartialTranscript('')
    try {
      await provider.stop()
    } catch {
      // ignore double-stop
    }
    // State returns to idle via the provider's onEnd once transcription
    // completes; nothing to do here synchronously.
  }, [])

  const reset = useCallback(() => {
    providerRef.current?.abort()
    setState('idle')
    setPartialTranscript('')
    setError(null)
  }, [])

  return { state, partialTranscript, error, supported, startRecording, stopRecording, reset }
}
