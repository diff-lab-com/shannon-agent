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
 */
export function createStubProvider(config: VoiceProviderConfig): VoiceProvider {
  const lang = config.lang ?? 'en-US'
  let handlers: StubHandlers | null = null
  let timer: ReturnType<typeof setTimeout> | null = null
  let idx = 0

  const tick = () => {
    if (!handlers) return
    handlers.onResult({ partial: PARTIALS[idx % PARTIALS.length], isFinal: false })
    idx += 1
    timer = setTimeout(tick, 800)
  }

  return {
    kind: 'stub',
    isSupported: () => true,
    start: async (next: StubHandlers) => {
      handlers = next
      idx = 0
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
