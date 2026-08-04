import type { VoiceProvider, VoiceProviderConfig } from './types'
import { createStubProvider } from './stubProvider'
import { createRemoteProvider } from './remoteProvider'
import { createLocalProvider } from './localProvider'

/**
 * Build a VoiceProvider from a config object. Falls back to the stub
 * provider when the requested kind isn't supported by the current runtime
 * (e.g. MediaRecorder unavailable in jsdom).
 */
export function createVoiceProvider(config: VoiceProviderConfig): VoiceProvider {
  switch (config.kind) {
    case 'remote': {
      const provider = createRemoteProvider(config)
      return provider.isSupported() ? provider : createStubProvider(config)
    }
    case 'local': {
      const provider = createLocalProvider(config)
      return provider.isSupported() ? provider : createStubProvider(config)
    }
    case 'stub':
    default:
      return createStubProvider(config)
  }
}

export function defaultVoiceConfig(): VoiceProviderConfig {
  // Cloud STT (via the Rust transcribe_audio command) is the primary path.
  // The factory falls back to the stub provider when MediaRecorder is
  // unavailable (jsdom tests, headless/older webviews). Callers that
  // want the local provider (P2-5e) override this in useVoice when the
  // user has `voiceLocalEnabled` set in Settings.
  return { kind: 'remote' }
}
