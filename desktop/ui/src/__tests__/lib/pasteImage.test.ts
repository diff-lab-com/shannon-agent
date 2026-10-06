// G3b P1-6/P1-4 — pure helpers behind the composer paste + extraction badge.
//
// imageFilesFromClipboard: only `kind === 'file'` + `image/*` items become
// files (a text paste never takes the image path). blobToBase64: strips the
// FileReader data-URL prefix. extractionMessageParams: report → i18n key.

import { describe, it, expect } from 'vitest'
import {
  imageFilesFromClipboard,
  blobToBase64,
  extractionMessageParams,
  PASTE_IMAGE_MIME_TO_EXT,
  MAX_PASTED_IMAGE_BYTES,
} from '@/lib/pasteImage'
import type { AttachmentExtractionReport } from '@/types'

function clipItem(kind: string, type: string, file: File | null) {
  return { kind, type, getAsFile: () => file }
}

describe('imageFilesFromClipboard', () => {
  it('collects image file items in clipboard order', () => {
    const png = new File([new Uint8Array([1])], 'a.png', { type: 'image/png' })
    const jpeg = new File([new Uint8Array([2])], 'b.jpg', { type: 'image/jpeg' })
    const data = {
      items: [
        clipItem('string', 'text/plain', null),
        clipItem('file', 'image/png', png),
        clipItem('file', 'application/pdf', new File([], 'x.pdf')), // non-image type
        clipItem('file', 'image/jpeg', jpeg),
        clipItem('file', 'image/webp', null), // null blob → skipped
      ],
    }
    const files = imageFilesFromClipboard(data)
    expect(files).toHaveLength(2)
    expect(files[0].name).toBe('a.png')
    expect(files[1].name).toBe('b.jpg')
  })

  it('text-only clipboard yields nothing', () => {
    expect(
      imageFilesFromClipboard({ items: [clipItem('string', 'text/plain', null)] }),
    ).toEqual([])
  })

  it('missing clipboard data yields nothing (never throws)', () => {
    expect(imageFilesFromClipboard(null)).toEqual([])
    expect(imageFilesFromClipboard({})).toEqual([])
    expect(imageFilesFromClipboard(undefined)).toEqual([])
  })
})

describe('blobToBase64', () => {
  it('reads a blob as bare base64 without the data-URL prefix', async () => {
    const bytes = new Uint8Array([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a])
    const b64 = await blobToBase64(new Blob([bytes], { type: 'image/png' }))
    expect(b64).not.toContain('data:')
    expect(b64).not.toContain(',')
    // Round-trip through atob to prove it is the same bytes.
    const decoded = Uint8Array.from(atob(b64), (c) => c.charCodeAt(0))
    expect([...decoded]).toEqual([...bytes])
  })
})

describe('extractionMessageParams', () => {
  const report = (over: Partial<AttachmentExtractionReport>): AttachmentExtractionReport => ({
    path: '/tmp/report.pdf',
    kind: 'pdf',
    extracted: true,
    sections_total: 0,
    sections_inlined: 0,
    truncated: false,
    ...over,
  })

  it('office reports carry the section counts', () => {
    const p = extractionMessageParams(report({ kind: 'docx', sections_total: 23, sections_inlined: 8 }))
    expect(p.id).toBe('chat.attach.extraction.office')
    expect(p.values).toMatchObject({ total: 23, inlined: 8 })
  })

  it('pdf reports split on truncation, no section values', () => {
    expect(extractionMessageParams(report({ kind: 'pdf', truncated: true })).id).toBe(
      'chat.attach.extraction.pdfTruncated',
    )
    const full = extractionMessageParams(report({ kind: 'pdf', truncated: false }))
    expect(full.id).toBe('chat.attach.extraction.pdfFull')
    expect(full.values).toBeUndefined()
  })
})

describe('paste limits table', () => {
  it('covers exactly the vision-accepted mime types and the shared 10 MiB cap', () => {
    expect(Object.keys(PASTE_IMAGE_MIME_TO_EXT).sort()).toEqual([
      'image/gif',
      'image/jpeg',
      'image/png',
      'image/webp',
    ])
    expect(MAX_PASTED_IMAGE_BYTES).toBe(10 * 1024 * 1024)
  })
})
