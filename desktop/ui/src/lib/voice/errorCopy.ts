// Voice error copy mapping — AI-chat-ux V1 (2026-10).
//
// The providers already normalize backend failures into stable error codes
// (`mapSttError` in remote/localProvider), but the composer used to toast
// the RAW message — an untranslated DOMException string like
// "Requested device not found" under a generic「语音输入出错」title. This
// module maps every provider code (plus the DOMException names embedded in
// mic failures) to an i18n key so the user gets an actionable, localized
// message; the raw text still rides along in the toast description for
// support/debugging.

import type { VoiceProviderError } from './types'

/** Codes a trip to Settings can actually fix — the toast grows a
 *  「去设置」action for these (V2). */
export const VOICE_ERROR_SETTINGS_CODES: ReadonlySet<string> = new Set([
  'not-configured',
  'invalid-key',
  'feature-disabled',
  'model-not-found',
])

/** Map a provider error to its localized-message i18n key. */
export function voiceErrorKey(err: VoiceProviderError): string {
  switch (err.code) {
    case 'not-configured':
      return 'voice.error.notConfigured'
    case 'invalid-key':
      return 'voice.error.invalidKey'
    case 'rate-limited':
      return 'voice.error.rateLimited'
    case 'network':
      return 'voice.error.network'
    case 'feature-disabled':
      return 'voice.error.featureDisabled'
    case 'model-not-found':
      return 'voice.error.modelNotFound'
    case 'model-loading':
      return 'voice.error.modelLoading'
    case 'audio-invalid':
      return 'voice.error.audioInvalid'
    case 'inference-failed':
      return 'voice.error.inferenceFailed'
    case 'language-unsupported':
      return 'voice.error.languageUnsupported'
    case 'unsupported':
      return 'voice.error.unsupported'
    case 'mic-denied': {
      // `mic-denied` covers every getUserMedia rejection; the DOMException
      // name embedded in the message splits permission vs hardware causes.
      const m = err.message
      if (/NotFoundError|Requested\s+device\s+not\s+found|DeviceNotFoundException/i.test(m)) {
        return 'voice.error.micNotFound'
      }
      if (/NotReadableError|TrackStartError|already\s+in\s+use/i.test(m)) {
        return 'voice.error.micUnavailable'
      }
      return 'voice.error.micDenied'
    }
    default:
      return 'voice.error.engineError'
  }
}
