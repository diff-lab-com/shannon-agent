// PM-12: stable per-message feedback key. Backend ChatMessage rows carry no
// id, so reactions are keyed by the message's timestamp plus a short content
// hash — stable across reloads, different for edited/re-sent text.
export function messageFeedbackKey(timestamp: number, content: string): string {
  return `${timestamp}:${hash32(content).toString(36)}`
}

/** FNV-1a 32-bit — tiny, deterministic, good enough for de-dup keys. */
export function hash32(input: string): number {
  let h = 0x811c9dc5
  for (let i = 0; i < input.length; i++) {
    h ^= input.charCodeAt(i)
    h = Math.imul(h, 0x01000193)
  }
  return h >>> 0
}
