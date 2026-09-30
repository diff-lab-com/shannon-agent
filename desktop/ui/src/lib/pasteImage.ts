/**
 * G3b P1-6 — composer clipboard-image paste helpers.
 *
 * The composer listens for `paste` and pulls `image/*` entries out of
 * `clipboardData.items` (screenshot tools and "Copy Image" put them there);
 * each is persisted backend-side via `save_pasted_image` and then rides the
 * regular attachment pipeline (absolute path → preflight → multimodal base64
 * block). Everything here is pure/structural so the extraction and filtering
 * are testable without a real clipboard event.
 */

import type { AttachmentExtractionReport } from '@/types'

/** Clipboard image MIME types the multimodal pipeline accepts → file extension. */
export const PASTE_IMAGE_MIME_TO_EXT: Record<string, string> = {
  'image/png': 'png',
  'image/jpeg': 'jpeg',
  'image/gif': 'gif',
  'image/webp': 'webp',
}

/** Shared image cap — mirrors `shannon_core::attachments::MAX_IMAGE_BYTES`. */
export const MAX_PASTED_IMAGE_BYTES = 10 * 1024 * 1024

/** Minimal structural shape of `DataTransfer` (keeps tests free of jsdom Event plumbing). */
export interface ClipboardDataLike {
  items?: ArrayLike<{ kind: string; type: string; getAsFile: () => File | null }>
}

/**
 * Collect the image files from a paste event's clipboard data. Non-image
 * items (plain text, files whose type is not `image/*`) are ignored, so a
 * regular text paste never takes the image path. Returns them in clipboard
 * order (screenshots arrive one per paste, but multi-image copies happen).
 */
export function imageFilesFromClipboard(data: ClipboardDataLike | null | undefined): File[] {
  const items = data?.items
  if (!items) return []
  const out: File[] = []
  for (const item of Array.from(items)) {
    if (item.kind !== 'file' || !item.type.startsWith('image/')) continue
    const file = item.getAsFile()
    if (file) out.push(file)
  }
  return out
}

/**
 * Read a blob as bare base64 (no `data:` URL prefix) — the exact form
 * `save_pasted_image` expects.
 */
export function blobToBase64(blob: Blob): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader()
    reader.onerror = () => reject(reader.error ?? new Error('FileReader failed'))
    reader.onload = () => {
      const result = typeof reader.result === 'string' ? reader.result : ''
      const comma = result.indexOf(',')
      resolve(comma >= 0 ? result.slice(comma + 1) : result)
    }
    reader.readAsDataURL(blob)
  })
}

/**
 * The chip/FileCard tooltip line for one extraction report, in i18n values.
 * Kept next to the paste helpers as the single place that decides which
 * message a report maps to (office sections vs pdf truncation vs failure).
 */
export function extractionMessageParams(
  report: AttachmentExtractionReport,
): { id: string; values?: Record<string, string | number> } {
  if (report.kind === 'pdf') {
    return report.truncated
      ? { id: 'chat.attach.extraction.pdfTruncated' }
      : { id: 'chat.attach.extraction.pdfFull' }
  }
  return {
    id: 'chat.attach.extraction.office',
    values: { total: report.sections_total, inlined: report.sections_inlined },
  }
}
