import { useState, useCallback, useRef, useEffect } from 'react'

export type VoiceState = 'idle' | 'recording' | 'transcribing' | 'speaking'

export interface UseVoiceOptions {
  onTranscript?: (text: string) => void
  simulateLatencyMs?: number
}

export interface UseVoiceResult {
  state: VoiceState
  partialTranscript: string
  error: string | null
  startRecording: () => Promise<void>
  stopRecording: () => Promise<void>
  speak: (text: string) => Promise<void>
  stopSpeaking: () => void
  reset: () => void
}

const STUB_PARTIALS = ['Listening...', 'Detected: hello world', 'Processing audio...']
const STUB_FINAL = 'This is a stub transcript. Real STT backend not configured.'

export function useVoice(options: UseVoiceOptions = {}): UseVoiceResult {
  const { onTranscript, simulateLatencyMs = 600 } = options
  const [state, setState] = useState<VoiceState>('idle')
  const [partialTranscript, setPartialTranscript] = useState('')
  const [error, setError] = useState<string | null>(null)
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null)
  const partialIdxRef = useRef(0)

  const clearTimer = () => {
    if (timerRef.current) {
      clearTimeout(timerRef.current)
      timerRef.current = null
    }
  }

  useEffect(() => () => clearTimer(), [])

  const startRecording = useCallback(async () => {
    setError(null)
    setState('recording')
    partialIdxRef.current = 0
    setPartialTranscript(STUB_PARTIALS[0])
  }, [])

  const stopRecording = useCallback(async () => {
    setState('transcribing')
    clearTimer()
    timerRef.current = setTimeout(() => {
      setPartialTranscript('')
      setState('idle')
      onTranscript?.(STUB_FINAL)
    }, simulateLatencyMs)
  }, [onTranscript, simulateLatencyMs])

  const speak = useCallback(async (_text: string) => {
    setError(null)
    setState('speaking')
  }, [])

  const stopSpeaking = useCallback(() => {
    setState('idle')
  }, [])

  const reset = useCallback(() => {
    clearTimer()
    setState('idle')
    setPartialTranscript('')
    setError(null)
  }, [])

  return { state, partialTranscript, error, startRecording, stopRecording, speak, stopSpeaking, reset }
}
