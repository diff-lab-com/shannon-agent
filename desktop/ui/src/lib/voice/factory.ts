import type { VoiceProvider, VoiceProviderConfig } from './types'
import { createStubProvider } from './stubProvider'
import { createRemoteProvider } from './remoteProvider'
import { createLocalProvider } from './localProvider'

/**
 * Build a VoiceProvider from a config object. Falls back to the stub
 * provider when the requested kind isn't supported by the current runtime
 * (e.g. MediaRecorder unavailable in jsdom). The stub reports
 * `isSupported() === false` (F-voice-gate), so an environment without the
 * real capture seams surfaces as unsupported to callers — `useVoice().supported`
 * is false and ChatInput hides the mic instead of offering a recording that
 * could only ever emit the stub's canned transcript.
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
  // When MediaRecorder is unavailable (jsdom tests, headless/older
  // webviews) the factory falls back to the stub provider, which reports
  // itself unsupported — the UI then hides the mic (F-voice-gate) instead
  // of rendering one that can only produce the stub transcript. Callers
  // that want the local provider (P2-5e) override this in useVoice when
  // the user has `voiceLocalEnabled` set in Settings.
  return { kind: 'remote' }
}
