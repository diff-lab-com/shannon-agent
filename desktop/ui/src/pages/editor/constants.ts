// Severity constants + normalizer for the Editor page. The normalizer is the
// same logic that lived inline in Editor.tsx — kept here so the orchestrator
// and sub-components share one source of truth when mapping server strings
// to the CodeMirror severity union.

import type { EditorDiagnostic } from '@/components/editor/CodeEditor'

export const SEVERITIES: EditorDiagnostic['severity'][] = [
  'error',
  'warning',
  'info',
  'hint',
]

export function normalizeSeverity(raw: string): EditorDiagnostic['severity'] {
  const lower = raw.toLowerCase()
  if (lower === 'error') return 'error'
  if (lower === 'warning') return 'warning'
  if (lower === 'info' || lower === 'information') return 'info'
  if (lower === 'hint') return 'hint'
  return 'warning'
}