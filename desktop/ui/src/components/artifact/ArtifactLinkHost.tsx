// ArtifactLinkHost — app-level glue between the open pipeline and the
// artifact dock (docs/plans/2026-09-25-desktop-chat-ui-open-and-artifact-
// design.md §4 P0-B / P1-C / P1-E).
//
// Two responsibilities, both event/registry based so no component has to
// thread props:
//   1. registers the panel router for external links — openLink('panel')
//      lands here and becomes a `web` artifact tab;
//   2. listens for `shannon:open-artifact-file` (FileRefChip clicks on
//      document/image extensions) and opens a disk-provenance artifact tab.
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
import { registerLinkPanelRouter } from '@/lib/openLink'
import { readTextFile } from '@/lib/tauri-api'
import { messageFor } from '@/i18n'
import { toast } from 'sonner'
import { errorMessage } from '@/lib/errorToast'
import { useArtifact, type ArtifactItem } from './ArtifactContext'
import type { ArtifactKind, DetectedArtifact } from './detectArtifact'

/** Open a disk file as a provenance-tagged artifact tab. */
export async function openDiskArtifact(
  open: (a: DetectedArtifact, opts?: { activate?: boolean }) => void,
  path: string,
  activate = true,
): Promise<ArtifactItem | null> {
  const ext = extOf(path)
  const title = basenameOf(path)
  const id = `disk:${path}`
  if (IMAGE_EXTS.has(ext)) {
    open({ kind: 'image', source: path, path, origin: 'disk', title, confidence: 'high', id }, { activate })
    return null
  }
  const kind: ArtifactKind = ARTIFACT_FILE_KINDS[ext] ?? 'other'
  try {
    const dto = await readTextFile(path)
    open({ kind, source: dto.content, path, origin: 'disk', title, confidence: 'high', id }, { activate })
    return null
  } catch (e) {
    const msg = errorMessage(e)
    if (msg.includes('too large') || msg.includes('binary')) {
      open({ kind: 'other', source: path, path, origin: 'disk', title, confidence: 'high', id }, { activate })
      return null
    }
    toast.error(messageFor('link.open.failed'), { description: msg })
    return null
  }
}

export function ArtifactLinkHost() {
  const { open } = useArtifact()

  // P1-E: external links default to a web tab (decision §5-6).
  useEffect(() => {
    registerLinkPanelRouter(url =>
      open({ kind: 'web', source: url, title: url, confidence: 'high' }),
    )
    return () => registerLinkPanelRouter(null)
  }, [open])

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
