// Voice level meter — AI-chat-ux V5 (2026-10).
//
// Taps the captured MediaStream with an AnalyserNode and reports an RMS
// loudness in 0..1 roughly ten times a second, so the composer's recording
// strip can show a live level cue (the user otherwise gets zero feedback
// between "started" and "stopped" — a dead-looking button). Pure utility:
// no React, fully detachable, and every failure (AudioContext unavailable,
// closed context) degrades to "no level events" rather than an error.

/** ~10 reports/second: responsive enough for a level cue, cheap enough to
 *  never show up in a profile. */
const LEVEL_INTERVAL_MS = 100

/**
 * Start sampling `stream`'s loudness. Returns a detach function that stops
 * the sampler and closes the AudioContext — idempotent, safe to call twice.
 */
export function attachLevelMeter(stream: MediaStream, onLevel: (level: number) => void): () => void {
  let detached = false
  const AudioContextCtor: typeof AudioContext | undefined =
    typeof window === 'undefined'
      ? undefined
      : window.AudioContext
        ?? (window as unknown as { webkitAudioContext?: typeof AudioContext }).webkitAudioContext
  if (!AudioContextCtor || stream.getAudioTracks().length === 0) {
    return () => { detached = true }
  }
  try {
    const ctx = new AudioContextCtor()
    const source = ctx.createMediaStreamSource(stream)
    const analyser = ctx.createAnalyser()
    analyser.fftSize = 512
    source.connect(analyser)
    const buf = new Float32Array(analyser.fftSize)
    const tick = () => {
      if (detached) return
      analyser.getFloatTimeDomainData(buf)
      let sum = 0
      for (let i = 0; i < buf.length; i++) sum += buf[i] * buf[i]
      const rms = Math.sqrt(sum / buf.length)
      // Perceptual lift: raw speech RMS sits around 0.02–0.3 — square-root
      // expansion spreads it across the 0..1 bar range.
      onLevel(Math.min(1, Math.sqrt(rms)))
    }
    const timer = window.setInterval(tick, LEVEL_INTERVAL_MS)
    tick()
    return () => {
      if (detached) return
      detached = true
      window.clearInterval(timer)
      try {
        source.disconnect()
        analyser.disconnect()
      } catch {
        /* context already closing */
      }
      void ctx.close().catch(() => { /* already closed */ })
    }
  } catch {
    // No AudioContext (exotic webview / permissions) — the recording is
    // still fine; only the level cue is absent.
    return () => { detached = true }
  }
}
