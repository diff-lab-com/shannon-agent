import type { VoiceProvider, VoiceProviderConfig } from './types'
import { createStubProvider } from './stubProvider'
import { createWebSpeechProvider } from './webSpeechProvider'
import { createRemoteProvider } from './remoteProvider'

/**
 * Build a VoiceProvider from a config object. Falls back to the stub
 * provider when the requested kind isn't supported by the current
 * runtime (e.g. Web Speech unavailable in Firefox, MediaRecorder
 * unavailable in jsdom).
 */
export function createVoiceProvider(config: VoiceProviderConfig): VoiceProvider {
  switch (config.kind) {
    case 'webspeech': {
      const provider = createWebSpeechProvider(config)
      return provider.isSupported() ? provider : createStubProvider(config)
    }
    case 'remote': {
      const provider = createRemoteProvider(config)
      return provider.isSupported() ? provider : createStubProvider(config)
    }
    case 'stub':
    default:
      return createStubProvider(config)
  }
}

export function defaultVoiceConfig(): VoiceProviderConfig {
  if (typeof window === 'undefined') return { kind: 'stub' }
  const w = window as unknown as {
    SpeechRecognition?: unknown
    webkitSpeechRecognition?: unknown
  }
  if (w.SpeechRecognition || w.webkitSpeechRecognition) return { kind: 'webspeech' }
  return { kind: 'stub' }
}
