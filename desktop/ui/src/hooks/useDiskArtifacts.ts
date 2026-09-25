// useDiskArtifacts — auto-dock disk artifacts written by file-mutating tools
// (docs/plans/2026-09-25-desktop-chat-ui-open-and-artifact-design.md §4 P1-C).
//
// Decision §5-2: the artifact tab always appears (discoverability), but
// whether it *activates* — yanking the dock open and switching tabs — is
// governed by the existing `shannon.artifact.autoOpen` setting (default
// off). Each completed tool call is processed exactly once, and re-writes
// of the same path replace the file's existing tab via the `disk:<path>` id.

import { useEffect, useRef } from 'react'
import { FILE_MUTATING_TOOLS, extractToolInputPath } from '@/lib/fileRefs'
import { ARTIFACT_FILE_KINDS, IMAGE_EXTS, extOf } from '@/lib/openFileRef'
import { openDiskArtifact } from '@/components/artifact/ArtifactLinkHost'
import { useArtifact } from '@/components/artifact/ArtifactContext'

const DISK_ARTIFACT_EXTS = new Set([
  ...Object.keys(ARTIFACT_FILE_KINDS),
  ...IMAGE_EXTS,
])

export function useDiskArtifacts(messages: { role: string; tool_calls?: { tool_use_id?: string; tool_name: string; tool_input?: unknown; status?: string; is_error?: boolean }[] }[] | null) {
  const { open, autoOpen } = useArtifact()
  const processedRef = useRef(new Set<string>())

  useEffect(() => {
    if (!messages) return
    for (const msg of messages) {
      if (msg.role !== 'assistant' || !msg.tool_calls) continue
      for (const tc of msg.tool_calls) {
        const key = tc.tool_use_id ?? `${tc.tool_name}:${tc.tool_input ? JSON.stringify(tc.tool_input).slice(0, 120) : ''}`
        if (processedRef.current.has(key)) continue
        if (tc.status !== 'completed' || tc.is_error) continue
        if (!FILE_MUTATING_TOOLS.has(tc.tool_name)) continue
        const path = extractToolInputPath(tc.tool_input)
        if (!path || !DISK_ARTIFACT_EXTS.has(extOf(path))) continue
        processedRef.current.add(key)
        void openDiskArtifact(open, path, autoOpen)
      }
    }
  }, [messages, open, autoOpen])
}
