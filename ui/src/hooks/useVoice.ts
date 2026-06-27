import { useState, useCallback, useRef, useEffect } from 'react'
import { createTtsSpeaker, type TtsSpeaker } from '@/lib/voice/tts'

export type VoiceState = 'idle' | 'recording' | 'transcribing' | 'speaking'

export interface UseVoiceOptions {
  onTranscript?: (text: string) => void
  simulateLatencyMs?: number
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

const STUB_PARTIALS = ['Listening...', 'Detected: hello world', 'Processing audio...']
const STUB_FINAL = 'This is a stub transcript. Real STT backend not configured.'

type SpeechRecognitionInstance = {
  lang: string
  continuous: boolean
  interimResults: boolean
  maxAlternatives: number
  start: () => void
  stop: () => void
  abort: () => void
  onresult: ((e: { resultIndex: number; results: { length: number; [i: number]: { 0: { transcript: string }; isFinal: boolean } } }) => void) | null
  onerror: ((e: { error?: string }) => void) | null
  onend: (() => void) | null
}

type AnySpeechRecognition = {
  new (): SpeechRecognitionInstance
}

function getSpeechRecognitionCtor(): AnySpeechRecognition | null {
  if (typeof window === 'undefined') return null
  const w = window as unknown as { SpeechRecognition?: AnySpeechRecognition; webkitSpeechRecognition?: AnySpeechRecognition }
  return w.SpeechRecognition ?? w.webkitSpeechRecognition ?? null
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
  const { onTranscript, simulateLatencyMs = 600, lang = 'en-US' } = options
  const [state, setState] = useState<VoiceState>('idle')
  const [partialTranscript, setPartialTranscript] = useState('')
  const [error, setError] = useState<string | null>(null)
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null)
  const partialIdxRef = useRef(0)
  const recognitionRef = useRef<SpeechRecognitionInstance | null>(null)
  const transcriptRef = useRef(onTranscript)
  transcriptRef.current = onTranscript
  const ttsRef = useRef<TtsSpeaker | null>(null)
  if (!ttsRef.current) {
    ttsRef.current = createTtsSpeaker({
      lang,
      onError: (msg) => setError(`Speech synthesis error: ${msg}`),
    })
  }

  const supported = getSpeechRecognitionCtor() !== null

  const clearTimer = () => {
    if (timerRef.current) {
      clearTimeout(timerRef.current)
      timerRef.current = null
    }
  }

  useEffect(() => {
    const speaker = ttsRef.current!
    claimSpeaker(speaker)
    return () => {
      clearTimer()
      recognitionRef.current?.abort()
      recognitionRef.current = null
      // Stop any in-flight TTS when the component unmounts.
      speaker.cancel()
      if (activeSpeaker === speaker) activeSpeaker = null
    }
  }, [])

  const startRecording = useCallback(async () => {
    setError(null)
    setState('recording')
    partialIdxRef.current = 0

    const Ctor = getSpeechRecognitionCtor()
    if (!Ctor) {
      setPartialTranscript(STUB_PARTIALS[0])
      return
    }

    try {
      const recognition = new Ctor()
      recognition.lang = lang
      recognition.continuous = true
      recognition.interimResults = true
      recognition.maxAlternatives = 1

      recognition.onresult = (e) => {
        let interim = ''
        for (let i = e.resultIndex; i < e.results.length; i++) {
          const result = e.results[i]
          if (result.isFinal) {
            const text = result[0].transcript.trim()
            if (text) transcriptRef.current?.(text)
          } else {
            interim += result[0].transcript
          }
        }
        setPartialTranscript(interim)
      }

      recognition.onerror = (e) => {
        if (e.error && e.error !== 'no-speech' && e.error !== 'aborted') {
          setError(`Voice recognition error: ${e.error}`)
        }
      }

      recognition.onend = () => {
        setState((s) => (s === 'recording' ? 'idle' : s))
      }

      recognitionRef.current = recognition
      recognition.start()
      setPartialTranscript('')
    } catch (err) {
      setError(String(err instanceof Error ? err.message : err))
      setState('idle')
    }
  }, [lang])

  const stopRecording = useCallback(async () => {
    const recognition = recognitionRef.current
    if (recognition) {
      setState('transcribing')
      try {
        recognition.stop()
      } catch {
        // ignore double-stop
      }
      setPartialTranscript('')
      setState('idle')
      return
    }

    setState('transcribing')
    clearTimer()
    timerRef.current = setTimeout(() => {
      setPartialTranscript('')
      setState('idle')
      transcriptRef.current?.(STUB_FINAL)
    }, simulateLatencyMs)
  }, [simulateLatencyMs])

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
    clearTimer()
    recognitionRef.current?.abort()
    recognitionRef.current = null
    setState('idle')
    setPartialTranscript('')
    setError(null)
  }, [])

  return { state, partialTranscript, error, supported, startRecording, stopRecording, speak, stopSpeaking, reset }
}
