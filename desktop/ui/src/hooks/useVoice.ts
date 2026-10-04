import { useState, useCallback, useRef, useEffect } from 'react'
import {
  createVoiceProvider,
  defaultVoiceConfig,
  type VoiceProvider,
  type VoiceProviderConfig,
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

// B1-5 P1-5: identity of the resolved STT config — everything the
// factory branches on, string-normalized (`null`/`undefined` both
// collapse to ''), so any provider/model/language change produces a
// new value.
function buildSignature(
  provider: 'cloud' | 'local',
  local?: UseVoiceOptions['local'],
): string {
  return [provider, local?.model ?? '', local?.language ?? ''].join('|')
}

function configFor(
  provider: 'cloud' | 'local',
  local?: UseVoiceOptions['local'],
): VoiceProviderConfig {
  return provider === 'local'
    ? { kind: 'local' as const, local: local ?? { model: null, language: null } }
    : defaultVoiceConfig()
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

  // Build the provider lazily on first render. The kind is resolved
  // from `options.provider` (P2-5e). When the local provider is
  // selected and MediaRecorder is unavailable, the factory falls back
  // to the stub — same fallback semantics as the cloud provider. The
  // stub reports `isSupported() === false` (F-voice-gate), so a
  // fallback environment surfaces here as `supported: false` and the
  // UI hides the mic.
  // B1-5 P1-5: this first build is only the starting point — ChatInput
  // renders before the config has loaded (cold start straight into
  // /chat), so it can resolve to plain cloud even for local-STT
  // users. Each build is stamped with the config signature and
  // rebuilt by the effect below whenever that signature drifts.
  const providerRef = useRef<VoiceProvider | null>(null)
  const builtSigRef = useRef<string | null>(null)
  if (!providerRef.current) {
    providerRef.current = createVoiceProvider(configFor(provider, local))
    builtSigRef.current = buildSignature(provider, local)
  }
  // `supported` is state rather than a per-render providerRef read so
  // a rebuilt provider re-reports it (the effect refreshes it on every
  // rebuild). Initialized from the first build so first-render output
  // is identical to the pre-B1-5 lazy-read behavior.
  const [supported, setSupported] = useState(() => providerRef.current?.isSupported() ?? false)

  const handleError = useCallback((err: VoiceProviderError) => {
    if (!err.silent) {
      setError(err.message)
      errorRef.current?.(err.message)
    }
  }, [])

  useEffect(() => {
    return () => { providerRef.current?.abort() }
  }, [])

  // B1-5 P1-5: rebuild the provider whenever the resolved config
  // changes (config arriving late after a cold start, or the user
  // flipping Settings → Voice). Deliberately dependency-free: a
  // change seen mid-capture is deferred, and every path back to
  // `idle` renders — so running on every commit behind a cheap
  // signature compare is the simplest wiring that can't miss the
  // post-idle rebuild.
  useEffect(() => {
    const signature = buildSignature(provider, local)
    if (builtSigRef.current === signature) return
    if (state !== 'idle') {
      // A capture is in flight — aborting here would cut off the
      // user's audio mid-recording. Skip now; the commit that lands
      // back on idle (onEnd) picks the rebuild up.
      return
    }
    providerRef.current?.abort()
    const next = createVoiceProvider(configFor(provider, local))
    providerRef.current = next
    builtSigRef.current = signature
    setSupported(next.isSupported())
  })

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
