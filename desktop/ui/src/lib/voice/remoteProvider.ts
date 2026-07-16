import type {
  VoiceProvider,
  VoiceProviderConfig,
  VoiceResultHandler,
  VoiceErrorHandler,
} from './types'
import { transcribeAudio } from '@/lib/tauri-api'

interface BoundHandlers {
  onResult: VoiceResultHandler
  onError: VoiceErrorHandler
  onEnd?: () => void
}

/**
 * Map a backend error message (which carries an `STT_*:` prefix) to a stable
 * provider error code the UI can switch on.
 */
function mapSttError(message: string): string {
  if (message.startsWith('STT_NOT_CONFIGURED')) return 'not-configured'
  if (message.startsWith('STT_INVALID_KEY')) return 'invalid-key'
  if (message.startsWith('STT_RATE_LIMITED')) return 'rate-limited'
  if (message.startsWith('STT_NETWORK')) return 'network'
  return 'engine-error'
}

/**
 * Strip the leading `STT_*:` machine prefix from a backend error so the
 * remaining message is presentable to the user.
 */
function cleanSttMessage(message: string): string {
  const idx = message.indexOf(':')
  if (idx >= 0 && /^[A-Z_]+$/.test(message.slice(0, idx))) {
    return message.slice(idx + 1).trim()
  }
  return message
}

/** Read a Blob as base64 (without the `data:...;base64,` prefix). */
function blobToBase64(blob: Blob): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader()
    reader.onloadend = () => {
      const result = reader.result
      if (typeof result !== 'string') {
        reject(new Error('Failed to read audio recording'))
        return
      }
      const comma = result.indexOf(',')
      resolve(comma >= 0 ? result.slice(comma + 1) : result)
    }
    reader.onerror = () => reject(reader.error ?? new Error('FileReader error'))
    reader.readAsDataURL(blob)
  })
}

/**
 * Cloud STT provider. Captures audio via `navigator.mediaDevices.getUserMedia`
 * + `MediaRecorder`, then base64-encodes the recording and sends it to the
 * Rust `transcribe_audio` command, which calls the configured
 * OpenAI-compatible Whisper endpoint (Groq / OpenAI / custom) and returns the
 * transcript text.
 *
 * API keys live server-side, so this provider only handles audio capture — no
 * endpoint URL or auth token is needed on the frontend.
 */
export function createRemoteProvider(_config: VoiceProviderConfig): VoiceProvider {
  let mediaRecorder: MediaRecorder | null = null
  let stream: MediaStream | null = null
  let handlers: BoundHandlers | null = null
  let chunks: Blob[] = []
  let aborted = false

  return {
    kind: 'remote',
    isSupported: () => {
      if (typeof window === 'undefined') return false
      if (!navigator?.mediaDevices?.getUserMedia) return false
      return typeof MediaRecorder !== 'undefined'
    },
    start: async (next: BoundHandlers) => {
      handlers = next
      aborted = false
      chunks = []
      if (!navigator.mediaDevices?.getUserMedia || typeof MediaRecorder === 'undefined') {
        next.onError({
          code: 'unsupported',
          message: 'Voice input is not supported in this environment',
        })
        return
      }
      try {
        stream = await navigator.mediaDevices.getUserMedia({ audio: true })
      } catch (err) {
        next.onError({
          code: 'mic-denied',
          message: String(err instanceof Error ? err.message : err),
        })
        return
      }
      const mimeType = MediaRecorder.isTypeSupported('audio/webm')
        ? 'audio/webm'
        : 'audio/ogg'
      mediaRecorder = new MediaRecorder(stream, { mimeType })
      mediaRecorder.ondataavailable = (e) => {
        if (e.data.size > 0) chunks.push(e.data)
      }
      mediaRecorder.onstop = () => {
        if (!handlers || aborted) return
        const blob = new Blob(chunks, { type: mimeType })
        void flush(blob, handlers)
      }
      mediaRecorder.start()
    },
    stop: async () => {
      aborted = false
      try { mediaRecorder?.stop() } catch { /* double-stop */ }
      stream?.getTracks().forEach((t) => t.stop())
    },
    abort: () => {
      aborted = true
      try { mediaRecorder?.stop() } catch { /* noop */ }
      stream?.getTracks().forEach((t) => t.stop())
      chunks = []
      handlers = null
    },
  }

  async function flush(blob: Blob, h: BoundHandlers) {
    try {
      const base64 = await blobToBase64(blob)
      const result = await transcribeAudio(base64, blob.type || 'audio/webm')
      if (result.text) {
        h.onResult({ transcript: result.text })
      } else {
        h.onError({ code: 'engine-error', message: 'Empty transcript returned' })
      }
      h.onEnd?.()
    } catch (err) {
      const message = String(err instanceof Error ? err.message : err)
      h.onError({ code: mapSttError(message), message: cleanSttMessage(message) })
    }
  }
}
