import { useState, useCallback, useRef, useEffect } from 'react'
import { createTtsSpeaker, type TtsSpeaker } from '@/lib/voice/tts'
import {
  createVoiceProvider,
  defaultVoiceConfig,
  type VoiceProvider,
  type VoiceProviderError,
} from '@/lib/voice'

export type VoiceState = 'idle' | 'recording' | 'transcribing' | 'speaking'

export interface UseVoiceOptions {
  onTranscript?: (text: string) => void
  /** Non-silent provider failures (rejected mic, bad key, network, …). */
  onError?: (message: string) => void
  lang?: string
}

export interface UseVoiceResult {
  state: VoiceState
  partialTranscript: string
  error: string | null
  supported: boolean
  startRecording: () => Promise<void>
  stopRecording: () => Promise<void>
  speak: (text: string) => Promise<void>
  stopSpeaking: () => void
  reset: () => void
}

// Track the active TTS speaker across hook instances so that a new
// useVoice mount cancels any utterance that is still playing. Without
// this, navigating away from a spoken assistant reply leaves the audio
// running in the background.
let activeSpeaker: TtsSpeaker | null = null
function claimSpeaker(speaker: TtsSpeaker) {
  if (activeSpeaker && activeSpeaker !== speaker) {
    activeSpeaker.cancel()
  }
  activeSpeaker = speaker
}

export function useVoice(options: UseVoiceOptions = {}): UseVoiceResult {
  const { onTranscript, onError, lang = 'en-US' } = options
  const [state, setState] = useState<VoiceState>('idle')
  const [partialTranscript, setPartialTranscript] = useState('')
  const [error, setError] = useState<string | null>(null)

  const transcriptRef = useRef(onTranscript)
  transcriptRef.current = onTranscript
  const errorRef = useRef(onError)
  errorRef.current = onError

  // Build the provider once. defaultVoiceConfig() picks cloud STT; the
  // factory falls back to the stub provider when MediaRecorder is unavailable.
  const providerRef = useRef<VoiceProvider | null>(null)
  if (!providerRef.current) {
    providerRef.current = createVoiceProvider(defaultVoiceConfig())
  }
  const supported = providerRef.current.isSupported()

  const ttsRef = useRef<TtsSpeaker | null>(null)
  if (!ttsRef.current) {
    ttsRef.current = createTtsSpeaker({
      lang,
      onError: (msg) => setError(`Speech synthesis error: ${msg}`),
    })
  }

  const handleError = useCallback((err: VoiceProviderError) => {
    if (!err.silent) {
      setError(err.message)
      errorRef.current?.(err.message)
    }
  }, [])

  useEffect(() => {
    const speaker = ttsRef.current!
    claimSpeaker(speaker)
    return () => {
      providerRef.current?.abort()
      speaker.cancel()
      if (activeSpeaker === speaker) activeSpeaker = null
    }
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

  const speak = useCallback(async (text: string) => {
    setError(null)
    const speaker = ttsRef.current!
    setState('speaking')
    if (!speaker.isSupported()) {
      setError('Speech synthesis not supported in this browser')
      return
    }
    speaker.speak(text)
  }, [])

  const stopSpeaking = useCallback(() => {
    ttsRef.current?.cancel()
    setState('idle')
  }, [])

  const reset = useCallback(() => {
    providerRef.current?.abort()
    setState('idle')
    setPartialTranscript('')
    setError(null)
  }, [])

  return { state, partialTranscript, error, supported, startRecording, stopRecording, speak, stopSpeaking, reset }
}
