// Shared diagnostic types for the Editor page. Co-located with the page so
// the orchestrator + sub-components see the same shape without round-tripping
// through @/components/editor/CodeEditor (which only knows about the bare
// EditorDiagnostic).

import type { EditorDiagnostic } from '@/components/editor/CodeEditor'

export interface AutoDiagnostic extends EditorDiagnostic {
  kind: 'auto'
  source?: string
  code?: string
}

export interface ManualDiagnostic extends EditorDiagnostic {
  kind: 'manual'
}

export type MixedDiagnostic = AutoDiagnostic | ManualDiagnostic

export interface DrawerDiag {
  file_path: string
  start_line: number
  start_character: number
  end_line: number
  end_character: number
  message: string
  language_id: string
}