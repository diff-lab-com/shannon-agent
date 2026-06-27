import type {
  VoiceProvider,
  VoiceProviderConfig,
  VoiceResultHandler,
  VoiceErrorHandler,
  VoiceProviderError,
} from './types'

interface BoundHandlers {
  onResult: VoiceResultHandler
  onError: VoiceErrorHandler
  onEnd?: () => void
}

interface RemoteChunk {
  sequence: number
  blob: Blob
}

interface RemoteResponse {
  transcript?: string
  partial?: string
  is_final?: boolean
  error?: string
}

/**
 * Remote STT provider. Captures audio via `navigator.mediaDevices.getUserMedia`,
 * chunks it at fixed intervals, and POSTs each chunk to a configurable
 * endpoint. The endpoint is expected to return `{ transcript, partial,
 * is_final }` JSON.
 *
 * This is a scaffold — production deployments need:
 *  - silence detection to avoid sending empty audio,
 *  - reconnect / backoff on network errors,
 *  - streaming WebSocket transport for sub-second latency.
 *
 * The current implementation accumulates chunks in memory and flushes
 * them on stop(); a real-time version would flush every ~250ms.
 */
export function createRemoteProvider(config: VoiceProviderConfig): VoiceProvider {
  const lang = config.lang ?? 'en-US'
  const endpoint = config.remoteEndpoint
  const authToken = config.remoteAuthToken

  let mediaRecorder: MediaRecorder | null = null
  let stream: MediaStream | null = null
  let handlers: BoundHandlers | null = null
  let chunks: Blob[] = []
  let sequence = 0
  let stopped = false

  const unsupported = (): VoiceProviderError => ({
    code: 'unsupported',
    message: 'MediaRecorder API not available in this environment',
  })

  return {
    kind: 'remote',
    isSupported: () => {
      if (typeof window === 'undefined' || !navigator?.mediaDevices?.getUserMedia) return false
      return typeof MediaRecorder !== 'undefined'
    },
    start: async (next: BoundHandlers) => {
      handlers = next
      stopped = false
      if (!endpoint) {
        next.onError({ code: 'no-endpoint', message: 'remoteEndpoint is required for remote voice provider' })
        return
      }
      if (!/^https:\/\//i.test(endpoint)) {
        next.onError({ code: 'insecure-protocol', message: 'remoteEndpoint must use https:// to protect auth token and audio payload' })
        return
      }
      if (!navigator.mediaDevices?.getUserMedia || typeof MediaRecorder === 'undefined') {
        next.onError(unsupported())
        return
      }
      try {
        stream = await navigator.mediaDevices.getUserMedia({ audio: true })
      } catch (err) {
        next.onError({
          code: 'mic-denied',
          message: String(err instanceof Error ? err.message : err),
          silent: true,
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
        if (!handlers || stopped) return
        const blob = new Blob(chunks, { type: mimeType })
        void flush(blob, handlers)
      }
      mediaRecorder.start()
    },
    stop: async () => {
      stopped = false
      try { mediaRecorder?.stop() } catch { /* double-stop */ }
      stream?.getTracks().forEach(t => t.stop())
    },
    abort: () => {
      stopped = true
      try { mediaRecorder?.stop() } catch { /* noop */ }
      stream?.getTracks().forEach(t => t.stop())
      chunks = []
      sequence = 0
      handlers = null
    },
  }

  async function flush(blob: Blob, h: BoundHandlers) {
    if (!endpoint) return
    const chunk: RemoteChunk = { sequence: sequence++, blob }
    void chunk
    try {
      const headers: Record<string, string> = {
        'Content-Type': blob.type || 'application/octet-stream',
        'X-Voice-Lang': lang,
      }
      if (authToken) headers['Authorization'] = `Bearer ${authToken}`
      const res = await fetch(endpoint, {
        method: 'POST',
        headers,
        body: blob,
      })
      if (!res.ok) {
        h.onError({ code: 'http-' + res.status, message: `Remote STT failed: ${res.status} ${res.statusText}` })
        return
      }
      const json = (await res.json()) as RemoteResponse
      if (json.error) {
        h.onError({ code: 'engine-error', message: json.error })
        return
      }
      if (json.is_final && json.transcript) {
        h.onResult({ transcript: json.transcript })
      } else if (json.partial) {
        h.onResult({ partial: json.partial, isFinal: false })
      }
      h.onEnd?.()
    } catch (err) {
      h.onError({
        code: 'network',
        message: String(err instanceof Error ? err.message : err),
      })
    }
  }
}
