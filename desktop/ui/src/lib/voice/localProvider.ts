import type {
  VoiceProvider,
  VoiceProviderConfig,
  VoiceResultHandler,
  VoiceErrorHandler,
} from './types'
import { transcribeAudioLocal, transcribeAudioLocalBase64 } from '@/lib/tauri-api'

interface BoundHandlers {
  onResult: VoiceResultHandler
  onError: VoiceErrorHandler
  onEnd?: () => void
}

interface LocalProviderOptions {
  /** Model slug to pass to `transcribe_audio_local`. `null` ⇒
   *  backend picks the smallest downloaded model. */
  model: string | null
  /** BCP-47 language hint. `null` ⇒ auto-detect. */
  language?: string | null
}

interface LocalVoiceConfig extends VoiceProviderConfig {
  local?: LocalProviderOptions
}

/**
 * Map a backend error message (which carries an `STT_*:` prefix)
 * to a stable provider error code the UI can switch on. Mirrors
 * `remoteProvider.ts::mapSttError` and adds the local-only codes
 * (`model-not-found`, `model-loading`, `audio-invalid`,
 * `inference-failed`, `feature-disabled`, `language-unsupported`).
 */
function mapSttError(message: string): string {
  if (message.startsWith('STT_NOT_CONFIGURED')) return 'not-configured'
  if (message.startsWith('STT_FEATURE_DISABLED')) return 'feature-disabled'
  if (message.startsWith('STT_MODEL_NOT_FOUND')) return 'model-not-found'
  if (message.startsWith('STT_MODEL_LOADING')) return 'model-loading'
  if (message.startsWith('STT_AUDIO_INVALID')) return 'audio-invalid'
  if (message.startsWith('STT_INFERENCE_FAILED')) return 'inference-failed'
  if (message.startsWith('STT_LANGUAGE_UNSUPPORTED')) return 'language-unsupported'
  if (message.startsWith('STT_INVALID_KEY')) return 'invalid-key'
  if (message.startsWith('STT_RATE_LIMITED')) return 'rate-limited'
  if (message.startsWith('STT_NETWORK')) return 'network'
  return 'engine-error'
}

/**
 * Strip the leading `STT_*:` machine prefix from a backend error
 * so the remaining message is presentable to the user.
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
 * Local STT provider. Captures audio via `MediaRecorder`,
 * base64-encodes the recording, and invokes the Rust
 * `transcribe_audio_local` command. The Rust side writes the
 * bytes to a temp WAV file and runs whisper-rs inference
 * locally. Audio never leaves the device.
 *
 * The MediaRecorder path: on Chromium ≥ 110 we ask for
 * `audio/wav` directly (whisper-rs's preferred format); older
 * webviews fall back to `audio/webm` / `audio/ogg` and the Rust
 * side will return `STT_AUDIO_INVALID` for non-WAV input — a
 * typed error the UI surfaces honestly.
 */
export function createLocalProvider(config: LocalVoiceConfig): VoiceProvider {
  const opts = config.local ?? { model: null }
  let mediaRecorder: MediaRecorder | null = null
  let stream: MediaStream | null = null
  let handlers: BoundHandlers | null = null
  let chunks: Blob[] = []
  let aborted = false

  return {
    kind: 'local',
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
          message: 'Local voice is not supported in this environment',
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
      // Prefer WAV when available; fall back gracefully. MediaRecorder
      // pick order matches the existing cloud provider so the user
      // gets a working recording on every webview.
      const mimeType = MediaRecorder.isTypeSupported('audio/wav')
        ? 'audio/wav'
        : MediaRecorder.isTypeSupported('audio/webm')
        ? 'audio/webm'
        : 'audio/ogg'
      mediaRecorder = new MediaRecorder(stream, { mimeType })
      mediaRecorder.ondataavailable = (e) => {
        if (e.data.size > 0) chunks.push(e.data)
      }
      mediaRecorder.onstop = () => {
        if (!handlers || aborted) return
        const blob = new Blob(chunks, { type: mimeType })
        void flush(blob, handlers, opts)
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

  async function flush(blob: Blob, h: BoundHandlers, o: LocalProviderOptions) {
    try {
      const base64 = await blobToBase64(blob)
      // The Rust command takes base64 + mime so it can pick the
      // right decoder (hound for WAV, fall back to a sensible
      // error for webm/ogg). Passing mime is necessary because
      // base64 alone doesn't carry the format.
      const result = await transcribeAudioLocalBase64(
        base64,
        blob.type || 'audio/webm',
        o.model,
        o.language ?? null,
      )
      if (result.text) {
        h.onResult({ transcript: result.text })
      } else {
        h.onError({ code: 'inference-failed', message: 'Empty transcript returned' })
      }
      h.onEnd?.()
    } catch (err) {
      const message = String(err instanceof Error ? err.message : err)
      h.onError({ code: mapSttError(message), message: cleanSttMessage(message) })
    }
  }
}

// Silence the unused warning for the path-based variant — it's
// still exported from tauri-api.ts for tests / power users who
// want to write the WAV file themselves (e.g. via the desktop's
// `fs` plugin). Marking the import as type-only would hide it
// from the file's public surface; a leading underscore does the
// same for the type-checker without changing the runtime shape.
void transcribeAudioLocal
