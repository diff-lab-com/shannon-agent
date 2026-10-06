// openFileRef — routes a clicked file reference to the right in-app surface
// (docs/plans/2026-09-25-desktop-chat-ui-open-and-artifact-design.md §4 P0-B).
//
// Document/artifact extensions go to the dock via the
// `shannon:open-artifact-file` event (the ArtifactLinkHost reads content and
// opens an artifact tab); everything else opens in the chat-inline editor
// via `shannon:open-code-file` (Chat.tsx owns that modal). Non-existent
// files never reach here — FileRefChip probes first — but the host still
// toasts a failure instead of failing silently.

import type { ArtifactKind } from '@/components/artifact/detectArtifact'

export const IMAGE_EXTS = new Set(['png', 'jpg', 'jpeg', 'gif', 'webp', 'bmp'])

/** Inline-renderable extensions → the artifact kind they open as. */
export const ARTIFACT_FILE_KINDS: Record<string, ArtifactKind> = {
  md: 'document',
  markdown: 'document',
  mdx: 'document',
  html: 'html',
  htm: 'html',
  svg: 'svg',
  mermaid: 'mermaid',
  mmd: 'mermaid',
}

export type OpenedFileRefSurface = 'artifact' | 'editor'

export function extOf(path: string): string {
  const base = path.slice(path.lastIndexOf('/') + 1)
  const idx = base.lastIndexOf('.')
  return idx > 0 ? base.slice(idx + 1).toLowerCase() : ''
}

function dispatchFileRefEvent(type: 'shannon:open-artifact-file' | 'shannon:open-code-file', path: string): void {
  window.dispatchEvent(new CustomEvent(type, { detail: { path } }))
}

/**
 * Route a file path to its in-app surface. Returns where it was sent so
 * callers (chips, menus, tests) can react.
 */
export function openFileRef(path: string): OpenedFileRefSurface {
  const ext = extOf(path)
  if (ARTIFACT_FILE_KINDS[ext] || IMAGE_EXTS.has(ext)) {
    dispatchFileRefEvent('shannon:open-artifact-file', path)
    return 'artifact'
  }
  dispatchFileRefEvent('shannon:open-code-file', path)
  return 'editor'
}
