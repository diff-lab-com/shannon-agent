import type {
  VoiceProvider,
  VoiceProviderConfig,
  VoiceResultHandler,
  VoiceErrorHandler,
  VoiceProviderError,
} from './types'

type SpeechRecognitionInstance = {
  lang: string
  continuous: boolean
  interimResults: boolean
  maxAlternatives: number
  start: () => void
  stop: () => void
  abort: () => void
  onresult: ((e: {
    resultIndex: number
    results: { length: number; [i: number]: { 0: { transcript: string }; isFinal: boolean } }
  }) => void) | null
  onerror: ((e: { error?: string }) => void) | null
  onend: (() => void) | null
}

type AnySpeechRecognition = { new (): SpeechRecognitionInstance }

interface BoundHandlers {
  onResult: VoiceResultHandler
  onError: VoiceErrorHandler
  onEnd?: () => void
}

function getCtor(): AnySpeechRecognition | null {
  if (typeof window === 'undefined') return null
  const w = window as unknown as {
    SpeechRecognition?: AnySpeechRecognition
    webkitSpeechRecognition?: AnySpeechRecognition
  }
  return w.SpeechRecognition ?? w.webkitSpeechRecognition ?? null
}

const IGNORED_ERRORS = new Set(['no-speech', 'aborted'])

/**
 * Web Speech API provider. Uses the browser-native SpeechRecognition
 * (Chrome / Edge / Safari). Not available in Firefox.
 */
export function createWebSpeechProvider(config: VoiceProviderConfig): VoiceProvider {
  const lang = config.lang ?? 'en-US'
  let recognition: SpeechRecognitionInstance | null = null

  return {
    kind: 'webspeech',
    isSupported: () => getCtor() !== null,
    start: async (handlers: BoundHandlers) => {
      const Ctor = getCtor()
      if (!Ctor) {
        const err: VoiceProviderError = {
          code: 'unsupported',
          message: 'SpeechRecognition not available in this browser',
        }
        handlers.onError(err)
        return
      }
      const rec = new Ctor()
      rec.lang = lang
      rec.continuous = true
      rec.interimResults = true
      rec.maxAlternatives = 1
      rec.onresult = (e) => {
        let interim = ''
        for (let i = e.resultIndex; i < e.results.length; i++) {
          const result = e.results[i]
          if (result.isFinal) {
            const transcript = result[0].transcript.trim()
            if (transcript) handlers.onResult({ transcript })
          } else {
            interim += result[0].transcript
          }
        }
        if (interim) handlers.onResult({ partial: interim, isFinal: false })
      }
      rec.onerror = (e) => {
        if (e.error && !IGNORED_ERRORS.has(e.error)) {
          handlers.onError({ code: e.error, message: `SpeechRecognition error: ${e.error}` })
        }
      }
      rec.onend = () => handlers.onEnd?.()
      recognition = rec
      rec.start()
    },
    stop: async () => {
      try { recognition?.stop() } catch { /* double-stop */ }
    },
    abort: () => {
      try { recognition?.abort() } catch { /* double-abort */ }
      recognition = null
    },
  }
}
