import { useState, useCallback, useRef, useEffect } from 'react'

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

type AnySpeechRecognition = {
  new (): {
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
}

function getSpeechRecognitionCtor(): AnySpeechRecognition | null {
  if (typeof window === 'undefined') return null
  const w = window as unknown as { SpeechRecognition?: AnySpeechRecognition; webkitSpeechRecognition?: AnySpeechRecognition }
  return w.SpeechRecognition ?? w.webkitSpeechRecognition ?? null
}

function getSpeechSynthesis(): SpeechSynthesis | null {
  if (typeof window === 'undefined') return null
  return window.speechSynthesis ?? null
}

export function useVoice(options: UseVoiceOptions = {}): UseVoiceResult {
  const { onTranscript, simulateLatencyMs = 600, lang = 'en-US' } = options
  const [state, setState] = useState<VoiceState>('idle')
  const [partialTranscript, setPartialTranscript] = useState('')
  const [error, setError] = useState<string | null>(null)
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null)
  const partialIdxRef = useRef(0)
  const recognitionRef = useRef<ReturnType<AnySpeechRecognition['new']> | null>(null)
  const transcriptRef = useRef(onTranscript)
  transcriptRef.current = onTranscript

  const supported = getSpeechRecognitionCtor() !== null

  const clearTimer = () => {
    if (timerRef.current) {
      clearTimeout(timerRef.current)
      timerRef.current = null
    }
  }

  useEffect(() => () => {
    clearTimer()
    recognitionRef.current?.abort()
    recognitionRef.current = null
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
    setState('speaking')
    const synth = getSpeechSynthesis()
    if (!synth) {
      setError('Speech synthesis not supported in this browser')
      return
    }
    const utterance = new SpeechSynthesisUtterance(text)
    utterance.lang = lang
    utterance.onend = () => setState('idle')
    utterance.onerror = () => setState('idle')
    synth.speak(utterance)
  }, [lang])

  const stopSpeaking = useCallback(() => {
    const synth = getSpeechSynthesis()
    if (synth) synth.cancel()
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
