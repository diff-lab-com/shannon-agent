import type {
  VoiceProvider,
  VoiceProviderConfig,
  VoiceResultHandler,
  VoiceErrorHandler,
} from './types'

interface StubHandlers {
  onResult: VoiceResultHandler
  onError: VoiceErrorHandler
  onEnd?: () => void
}

const PARTIALS = ['Listening...', 'Detected: hello world', 'Processing audio...']
const FINAL = 'This is a stub transcript. Real STT backend not configured.'

/**
 * No-op provider for environments without Web Speech (e.g. jsdom tests,
 * privacy mode). Emits a deterministic partial sequence then a final
 * string when stop() is called.
 *
 * F-voice-gate: the stub is only ever produced as a FALLBACK for
 * environments without real STT support (see the factory), so it reports
 * `isSupported() === false` — honestly. Reporting true here used to keep
 * ChatInput's `voice.supported` gate permanently open, rendering a mic
 * that could only ever emit this canned transcript. The start/stop
 * methods stay functional so jsdom tests can drive the hook directly.
 */
export function createStubProvider(config: VoiceProviderConfig): VoiceProvider {
  const lang = config.lang ?? 'en-US'
  let handlers: StubHandlers | null = null
  let timer: ReturnType<typeof setTimeout> | null = null

  return {
    kind: 'stub',
    isSupported: () => false,
    start: async (next: StubHandlers) => {
      handlers = next
      // Emit one immediate partial so the UI shows life, then idle.
      next.onResult({ partial: PARTIALS[0], isFinal: false })
      void lang
    },
    stop: async () => {
      if (timer) {
        clearTimeout(timer)
        timer = null
      }
      handlers?.onResult({ transcript: FINAL })
      handlers?.onEnd?.()
    },
    abort: () => {
      if (timer) {
        clearTimeout(timer)
        timer = null
      }
      handlers = null
    },
  }
}
