// ArtifactLinkHost — app-level glue between the open pipeline and the
// artifact dock (docs/plans/2026-09-25-desktop-chat-ui-open-and-artifact-
// design.md §4 P0-B / P1-C).
//
// Listens for `shannon:open-artifact-file` (FileRefChip clicks on
// document/image extensions) and opens a disk-provenance artifact tab.
// The panel router for external links lives in RightDock instead — the
// dock only exists on the chat page, and routing a link into an invisible
// tab from Settings would look like a dead click (§review P1-2).
//
// Disk reads are capped by the backend; oversized/binary files degrade to
// an `other` tab whose card offers the OS default app and folder reveal
// (§4 P1-D) instead of failing silently.

import { useEffect } from 'react'
import {
  ARTIFACT_FILE_KINDS,
  IMAGE_EXTS,
  extOf,
} from '@/lib/openFileRef'
import { basenameOf } from '@/lib/fileRefs'
import { readTextFile, readTextFileErrorCode } from '@/lib/tauri-api'
import { messageFor } from '@/i18n'
import { toast } from 'sonner'
import { errorMessage } from '@/lib/errorToast'
import { useArtifact } from './ArtifactContext'
import type { ArtifactKind, DetectedArtifact } from './detectArtifact'

/** Open a disk file as a provenance-tagged artifact tab. */
export async function openDiskArtifact(
  open: (a: DetectedArtifact, opts?: { activate?: boolean }) => void,
  path: string,
  activate = true,
): Promise<void> {
  const ext = extOf(path)
  const title = basenameOf(path)
  const id = `disk:${path}`
  if (IMAGE_EXTS.has(ext)) {
    open({ kind: 'image', source: path, path, origin: 'disk', title, confidence: 'high', id }, { activate })
    return
  }
  const kind: ArtifactKind = ARTIFACT_FILE_KINDS[ext] ?? 'other'
  try {
    const dto = await readTextFile(path)
    open({ kind, source: dto.content, path, origin: 'disk', title, confidence: 'high', id }, { activate })
  } catch (e) {
    // §P2-24: branch on the structured error code, not English substrings —
    // these three codes mean "readable, just not inline-renderable", so the
    // tab degrades to the OS-handoff card instead of a failure toast.
    const code = readTextFileErrorCode(e)
    if (code === 'file_too_large' || code === 'binary_file' || code === 'not_utf8') {
      open({ kind: 'other', source: path, path, origin: 'disk', title, confidence: 'high', id }, { activate })
      return
    }
    toast.error(messageFor('link.open.failed'), { description: errorMessage(e) })
  }
}

export function ArtifactLinkHost() {
  const { open } = useArtifact()

  // P0-B: file chips on document/image extensions dispatch this event.
  useEffect(() => {
    const onOpen = (e: Event) => {
      const path = (e as CustomEvent<{ path?: string }>).detail?.path
      if (path) void openDiskArtifact(open, path)
    }
    window.addEventListener('shannon:open-artifact-file', onOpen)
    return () => window.removeEventListener('shannon:open-artifact-file', onOpen)
  }, [open])

  return null
}
