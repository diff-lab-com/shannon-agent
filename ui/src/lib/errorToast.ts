import { toast } from 'sonner'

/**
 * Extract a human-readable message from an unknown catch value.
 *
 * Tauri command rejections come back as strings (Tauri serializes the
 * error), plain Errors carry `.message`, and unexpected throws may be
 * anything. This helper normalises all of them so the user sees the
 * actual cause instead of a generic "Failed" toast.
 */
export function errorMessage(e: unknown): string {
  if (e instanceof Error) return e.message
  if (typeof e === 'string') return e
  if (e && typeof e === 'object' && 'message' in e) {
    const msg = (e as { message: unknown }).message
    if (typeof msg === 'string') return msg
  }
  return String(e)
}

/**
 * Toast an i18n-keyed failure with the actual error cause in the
 * description slot. Drops the silent `console.warn` pattern — the
 * toast itself is the user-visible signal, and devtools still captures
 * the original via the description string.
 */
export function toastError(translation: string, e: unknown): void {
  toast.error(translation, { description: errorMessage(e) })
}
